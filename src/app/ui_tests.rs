//! Headless UI integration tests: the real `IsengardApp` in a GPUI Kit test
//! window, driven by keystrokes and clicks (`gpui_kit::test`).
//!
//! Imports are explicit (no `gpui_kit::*` glob): with `test-support` on, the
//! glob brings in GPUI's `test` attribute.
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::{
    AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowHandle, WindowOptions,
    base::Root, px, size, test::TestWindowExt as _,
};

use super::{AgentPanelEvent, IsengardApp};
use crate::acp::testing::FakeAgent;
use crate::acp::{FileRead, FileWrite};
use crate::config::AppConfig;

struct Harness {
    window: WindowHandle<Root>,
    app: Entity<IsengardApp>,
    // Keeps the fixture files alive for the test's duration.
    dir: tempfile::TempDir,
}

impl Harness {
    fn new(cx: &mut TestAppContext) -> Self {
        let mut config = AppConfig::default();
        // Never launch a real agent (Claude may be installed on the dev box).
        config.agent.command = Some("/nonexistent/isengard-test-agent".into());
        Self::with_config(config, cx)
    }

    fn with_config(config: AppConfig, cx: &mut TestAppContext) -> Self {
        let menu_bar = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::editor::highlights::init();
            super::init(cx);
            crate::menus::init(cx)
        });
        let (window, app) = cx.update(|cx| {
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(800.)),
                    cx,
                ))),
                ..Default::default()
            };
            let (window, app) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| IsengardApp::new(config, None, menu_bar, window, cx))
            })
            .expect("open test window");
            (window.downcast::<Root>().expect("Kit Root"), app)
        });
        Self {
            window,
            app,
            dir: tempfile::tempdir().expect("temp dir"),
        }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&path, text).expect("write fixture");
        path
    }

    /// Set the agent composer text and refresh `@` mentions (does not rely on
    /// keyboard focus — `TextareaState::set_value` skips `InputEvent::Change`).
    fn agent_composer(&self, text: &str, cx: &mut TestAppContext) {
        self.with_app(cx, |app, window, cx| {
            let panel = app.agent.clone().expect("agent panel");
            panel.update(cx, |panel, cx| {
                panel.set_composer_text_for_test(text, window, cx);
            });
        });
    }

    fn agent_mention_labels(&self, cx: &mut TestAppContext) -> Vec<String> {
        cx.read(|cx| {
            self.app
                .read(cx)
                .agent
                .as_ref()
                .unwrap()
                .read(cx)
                .mention_labels()
        })
    }

    /// Opens `path` as a pinned tab (focuses its editor, like a double-click).
    fn open(&self, path: &Path, cx: &mut TestAppContext) {
        self.with_app(cx, |app, window, cx| app.open_file(path, true, window, cx));
    }

    fn with_app<R>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut IsengardApp, &mut gpui_kit::Window, &mut gpui_kit::Context<IsengardApp>) -> R,
    ) -> R {
        let app = self.app.clone();
        let out = cx
            .update_window(self.window.into(), |_, window, cx| {
                app.update(cx, |app, cx| f(app, window, cx))
            })
            .expect("window alive");
        cx.run_until_parked();
        out
    }

    /// Types into whatever has focus, then lets subscriptions run.
    fn input(&self, text: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window.into(), |_, window, cx| window.input(text, cx))
            .expect("window alive");
        cx.run_until_parked();
    }

    fn press(&self, keys: &'static str, cx: &mut TestAppContext) {
        cx.update_window(self.window.into(), |_, window, cx| window.press(keys, cx))
            .expect("window alive");
        cx.run_until_parked();
    }

    fn click(&self, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(self.window.into(), |_, window, cx| window.click(id, cx))
            .expect("window alive");
        cx.run_until_parked();
    }

    fn has(&self, id: &'static str, cx: &mut TestAppContext) -> bool {
        cx.update_window(self.window.into(), |_, window, cx| {
            // Observe the current state, not the last frame painted.
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .expect("window alive")
    }

    /// `(is_modified, buffer text)` for the tab at `path`, or `None` if closed.
    fn tab(&self, path: &Path, cx: &mut TestAppContext) -> Option<(bool, String)> {
        cx.read(|cx| {
            let app = self.app.read(cx);
            let tab = app.tabs.iter().find(|t| t.path == path)?;
            let text = tab.editor_state()?.read(cx).value().to_string();
            Some((tab.is_modified, text))
        })
    }
}

#[gpui_kit::test]
fn typing_marks_dirty_and_undo_back_to_saved_clears_it(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "hello");
    h.open(&path, cx);
    assert_eq!(h.tab(&path, cx), Some((false, "hello".into())));

    h.input("X", cx);
    assert_eq!(h.tab(&path, cx), Some((true, "Xhello".into())));

    h.press("secondary-z", cx);
    assert_eq!(h.tab(&path, cx), Some((false, "hello".into())));
}

#[gpui_kit::test]
fn save_writes_disk_and_clears_dirty(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "hello");
    h.open(&path, cx);
    h.input("X", cx);

    h.press("secondary-s", cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Xhello");
    assert_eq!(h.tab(&path, cx), Some((false, "Xhello".into())));

    // The saved text is the new baseline: undoing past the save is dirty again.
    h.press("secondary-z", cx);
    assert_eq!(h.tab(&path, cx), Some((true, "hello".into())));
}

#[gpui_kit::test]
fn save_all_saves_the_rest_when_one_file_fails(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let a = h.file("a.txt", "a");
    let b = h.file("b.txt", "b");
    h.open(&a, cx);
    h.input("1", cx);
    h.open(&b, cx);
    h.input("2", cx);

    // `a.txt` becomes a directory, so writing it fails.
    std::fs::remove_file(&a).unwrap();
    std::fs::create_dir(&a).unwrap();

    let after_ran = Rc::new(Cell::new(false));
    let flag = after_ran.clone();
    h.with_app(cx, move |app, window, cx| {
        app.request_save_all(move |_, _, _| flag.set(true), None, window, cx);
    });

    assert_eq!(std::fs::read_to_string(&b).unwrap(), "2b", "b saved past a's failure");
    assert_eq!(h.tab(&b, cx).map(|t| t.0), Some(false));
    assert_eq!(h.tab(&a, cx).map(|t| t.0), Some(true), "failed file stays dirty");
    assert!(!after_ran.get(), "close/quit/switch must not run after a failure");
}

#[gpui_kit::test]
fn save_all_continues_when_untitled_write_fails_after_dialog(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let root = h.dir.path().to_path_buf();
    h.open_folder(&root, cx);
    let disk = h.file("disk.txt", "disk");
    h.open(&disk, cx);
    h.input("!", cx);

    let untitled = h.with_app(cx, |app, window, cx| {
        app.open_untitled(Some(root.clone()), window, cx);
        app.tabs.active().map(|t| t.path.clone()).expect("untitled tab")
    });
    h.input("scratch", cx);
    assert!(h.tab(&untitled, cx).is_some_and(|(dirty, _)| dirty));

    // Path that cannot be a file (directory) — same as post-dialog write failure.
    let bad = root.join("not-a-file");
    std::fs::create_dir(&bad).unwrap();

    let after_ran = Rc::new(Cell::new(false));
    let flag = after_ran.clone();
    let untitled_q = untitled.clone();
    let disk_q = disk.clone();
    h.with_app(cx, move |app, window, cx| {
        // Same path as prompt_save_untitled's on_write_fail (skip native picker).
        let mut failures = Vec::new();
        if let Err(err) = app.materialize_untitled(&untitled_q, bad, window, cx) {
            failures.push(format!(
                "{}: {err:#}",
                crate::editor::document::tab_label(&untitled_q)
            ));
        }
        let after: super::AfterSaveAll = Rc::new(move |_, _, _| flag.set(true));
        app.request_save_queue(vec![disk_q], failures, after, None, window, cx);
    });

    assert_eq!(
        std::fs::read_to_string(&disk).unwrap(),
        "!disk",
        "on-disk tab still saves after untitled write failure"
    );
    assert_eq!(h.tab(&disk, cx).map(|t| t.0), Some(false));
    assert!(
        h.tab(&untitled, cx).is_some_and(|(dirty, text)| dirty && text.contains("scratch")),
        "untitled stays open and dirty"
    );
    assert!(!after_ran.get(), "after must not run when any save failed");
}

#[gpui_kit::test]
fn closing_a_clean_tab_needs_no_confirmation(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "hello");
    h.open(&path, cx);

    h.press("secondary-w", cx);
    assert_eq!(h.tab(&path, cx), None);
    assert!(!h.has("dialog-secondary", cx));
}

#[gpui_kit::test]
fn closing_a_dirty_tab_asks_and_dont_save_discards(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "hello");
    h.open(&path, cx);
    h.input("X", cx);

    h.press("secondary-w", cx);
    assert!(h.has("dialog-secondary", cx), "unsaved-changes dialog is open");
    assert!(h.tab(&path, cx).is_some(), "tab stays open until answered");

    h.click("dialog-secondary", cx);
    assert_eq!(h.tab(&path, cx), None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello", "nothing written");
}

#[gpui_kit::test]
fn closing_a_dirty_tab_then_save_writes_and_closes(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "hello");
    h.open(&path, cx);
    h.input("X", cx);

    h.press("secondary-w", cx);
    h.click("dialog-primary", cx);
    assert_eq!(h.tab(&path, cx), None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Xhello");
}

impl Harness {
    fn folder(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::create_dir_all(&path).expect("create folder");
        path
    }

    fn open_folder(&self, path: &Path, cx: &mut TestAppContext) {
        self.with_app(cx, |app, window, cx| app.open_folder(path, window, cx));
    }

    fn agent_visible(&self, cx: &mut TestAppContext) -> bool {
        cx.read(|cx| self.app.read(cx).agent_visible)
    }

    fn agent_event(&self, event: AgentPanelEvent, cx: &mut TestAppContext) {
        self.with_app(cx, |app, window, cx| {
            let panel = app.agent.clone().expect("agent panel");
            app.handle_agent_event(&panel, &event, window, cx);
        });
    }

    fn transcript(&self, cx: &mut TestAppContext) -> Vec<String> {
        cx.read(|cx| {
            let app = self.app.read(cx);
            app.agent.as_ref().map(|a| a.read(cx).transcript(cx)).unwrap_or_default()
        })
    }

    /// Pump GPUI until `done` holds; the fake agent answers from its own thread.
    fn wait_for(&self, cx: &mut TestAppContext, what: &str, done: impl Fn(&mut TestAppContext) -> bool) {
        for _ in 0..500 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            cx.executor().advance_clock(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    fn wait_ready(&self, cx: &mut TestAppContext) {
        let app = self.app.clone();
        self.wait_for(cx, "agent ready", move |cx| {
            cx.read(|cx| app.read(cx).agent.as_ref().is_some_and(|a| a.read(cx).is_ready()))
        });
    }

    fn ask(&self, text: &'static str, cx: &mut TestAppContext) {
        self.input(text, cx);
        self.press("enter", cx);
        let app = self.app.clone();
        let reply = format!("agent: echo: {text}");
        self.wait_for(cx, "agent reply", move |cx| {
            cx.read(|cx| {
                app.read(cx).agent.as_ref().is_some_and(|a| {
                    let a = a.read(cx);
                    a.is_ready() && a.transcript(cx).contains(&reply)
                })
            })
        });
    }

    fn saved_chats(&self, folder: &Path, cx: &mut TestAppContext) -> (Vec<String>, Option<String>) {
        cx.read(|cx| {
            let session = self
                .app
                .read(cx)
                .config
                .session(&folder.to_string_lossy())
                .cloned()
                .unwrap_or_default();
            (
                session.agent_chats.into_iter().map(|c| c.id).collect(),
                session.agent_active,
            )
        })
    }
}

#[gpui_kit::test]
fn agent_panel_toggles_and_is_remembered_per_folder(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let a = h.folder("a");
    let b = h.folder("b");
    h.open_folder(&a, cx);
    assert!(!h.agent_visible(cx), "hidden by default");

    h.press("secondary-shift-b", cx);
    assert!(h.agent_visible(cx));
    assert!(h.has("agent-hide", cx));

    h.open_folder(&b, cx);
    assert!(!h.agent_visible(cx), "a new folder starts hidden");
    assert!(!h.has("agent-hide", cx));

    h.open_folder(&a, cx);
    assert!(h.agent_visible(cx), "restored with the folder's session");

    h.press("secondary-shift-b", cx);
    assert!(!h.agent_visible(cx));
}

#[gpui_kit::test]
fn tree_move_relocates_files_and_remaps_open_tabs(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let file = h.file("a.txt", "hello");
    let inner = h.file("dir/inner.txt", "inner");
    let dst = h.folder("dst");
    h.open_folder(h.dir.path(), cx);
    h.open(&file, cx);
    h.open(&inner, cx);

    // File into a folder: moves on disk, open tab follows.
    let moved = dst.join("a.txt");
    let r = h.with_app(cx, |app, w, cx| app.move_path(&file, &dst, w, cx));
    assert!(r.is_ok(), "{r:?}");
    assert!(!file.exists() && moved.exists());
    assert!(h.tab(&moved, cx).is_some() && h.tab(&file, cx).is_none());

    // Folder with an open tab inside.
    let dir = h.dir.path().join("dir");
    let r = h.with_app(cx, |app, w, cx| app.move_path(&dir, &dst, w, cx));
    assert!(r.is_ok(), "{r:?}");
    assert!(h.tab(&dst.join("dir/inner.txt"), cx).is_some());

    // Refusals: into own subtree, name collision, workspace root.
    let moved_dir = dst.join("dir");
    let r = h.with_app(cx, |app, w, cx| app.move_path(&dst, &moved_dir, w, cx));
    assert!(r.is_err());
    h.file("collide/a.txt", "x");
    let collide = h.dir.path().join("collide");
    let r = h.with_app(cx, |app, w, cx| app.move_path(&collide.join("a.txt"), &dst, w, cx));
    assert!(r.is_err());
    let root = h.dir.path().to_path_buf();
    let r = h.with_app(cx, |app, w, cx| app.move_path(&root, &dst, w, cx));
    assert!(r.is_err());
}

#[test]
fn reorder_allowed_skips_noop_drops() {
    use crate::ui::file_tree::reorder_allowed;
    use std::path::{Path, PathBuf};
    let roots: Vec<PathBuf> = ["/a", "/b", "/c"].iter().map(PathBuf::from).collect();
    assert!(reorder_allowed(&roots, Path::new("/c"), Some(Path::new("/a"))));
    assert!(reorder_allowed(&roots, Path::new("/a"), None));
    assert!(!reorder_allowed(&roots, Path::new("/c"), None));
    assert!(!reorder_allowed(&roots, Path::new("/a"), Some(Path::new("/b"))));
    assert!(!reorder_allowed(&roots, Path::new("/a"), Some(Path::new("/a"))));
}

#[test]
fn move_allowed_rejects_noop_and_cyclic_moves() {
    use crate::ui::file_tree::move_allowed;
    use std::path::Path;
    assert!(move_allowed(Path::new("/w/a.txt"), Path::new("/w/dst")));
    assert!(!move_allowed(Path::new("/w/a"), Path::new("/w/a")));
    assert!(!move_allowed(Path::new("/w/a"), Path::new("/w/a/b")));
    assert!(!move_allowed(Path::new("/w/a/x.txt"), Path::new("/w/a")));
}

#[gpui_kit::test]
fn agent_reads_unsaved_buffer_and_writes_reload_the_tab(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "one\ntwo\nthree\n");
    h.open_folder(h.dir.path(), cx);
    h.open(&path, cx);
    h.input("X", cx);

    let (reply, answer) = flume::bounded(1);
    h.agent_event(
        AgentPanelEvent::ReadFile(FileRead {
            path: path.clone(),
            line: Some(1),
            limit: Some(1),
            reply,
        }),
        cx,
    );
    assert_eq!(answer.try_recv().unwrap(), Ok("Xone\n".into()), "open buffer wins over disk");

    let (reply, answer) = flume::bounded(1);
    h.agent_event(
        AgentPanelEvent::WriteFile(FileWrite {
            path: path.clone(),
            content: "agent\n".into(),
            reply,
        }),
        cx,
    );
    assert_eq!(answer.try_recv().unwrap(), Ok(()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "agent\n");
    assert_eq!(h.tab(&path, cx), Some((false, "agent\n".into())), "tab shows saved agent text");

    let (reply, answer) = flume::bounded(1);
    h.agent_event(
        AgentPanelEvent::ReadFile(FileRead {
            path: PathBuf::from("relative.txt"),
            line: None,
            limit: None,
            reply,
        }),
        cx,
    );
    assert!(answer.try_recv().unwrap().is_err(), "relative paths are rejected");
}

#[gpui_kit::test]
fn agent_writes_show_file_change_summary(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("notes.txt", "alpha\nbeta\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);

    let (reply, answer) = flume::bounded(1);
    h.agent_event(
        AgentPanelEvent::WriteFile(FileWrite {
            path: path.clone(),
            content: "alpha\nBETA\ngamma\n".into(),
            reply,
        }),
        cx,
    );
    assert_eq!(answer.try_recv().unwrap(), Ok(()));

    let panel = cx.read(|cx| h.app.read(cx).agent.clone()).unwrap();
    panel.update(cx, |panel, cx| panel.flush_file_changes_for_test(cx));
    let summary = cx.read(|cx| panel.read(cx).file_change_summaries_for_test());
    assert_eq!(summary, vec![("notes.txt".into(), 2, 1, false)]);
}

#[gpui_kit::test]
fn agent_file_change_cards_survive_transcript_reload(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("notes.txt", "alpha\nbeta\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);

    let panel = cx.read(|cx| h.app.read(cx).agent.clone()).unwrap();
    panel.update(cx, |panel, cx| {
        panel.seed_chat_session_for_test("chat-1");
        panel.push_user_for_test("please edit", cx);
    });

    let (reply, answer) = flume::bounded(1);
    h.agent_event(
        AgentPanelEvent::WriteFile(FileWrite {
            path: path.clone(),
            content: "alpha\nBETA\ngamma\n".into(),
            reply,
        }),
        cx,
    );
    assert_eq!(answer.try_recv().unwrap(), Ok(()));

    panel.update(cx, |panel, cx| panel.flush_file_changes_for_test(cx));
    assert_eq!(
        cx.read(|cx| panel.read(cx).file_change_summaries_for_test()),
        vec![("notes.txt".into(), 2, 1, false)]
    );
    assert_eq!(
        cx.read(|cx| panel.read(cx).chats()[0].file_changes.len()),
        1
    );

    // Cold-start race: SessionOpened can restore before replay User bubbles.
    panel.update(cx, |panel, cx| {
        panel.clear_entries_for_test(cx);
        panel.restore_file_change_cards_for_test(cx);
        assert_eq!(panel.file_change_card_count_for_test(), 0);
        panel.push_user_for_test("please edit", cx);
        panel.restore_file_change_cards_for_test(cx);
    });
    assert_eq!(cx.read(|cx| panel.read(cx).file_change_card_count_for_test()), 1);
    assert_eq!(
        cx.read(|cx| panel.read(cx).file_change_summaries_for_test()),
        vec![("notes.txt".into(), 2, 1, false)]
    );

    // The splice adds a row; the transcript list must count it, or the newest
    // replies are cut off after a restart.
    panel.update(cx, |panel, cx| {
        panel.push_user_for_test("and then?", cx);
        panel.restore_file_change_cards_for_test(cx);
    });
    let (entries, rows) = cx.read(|cx| panel.read(cx).row_counts_for_test(cx));
    assert_eq!(entries, rows, "transcript list shows every entry");
}

#[gpui_kit::test]
fn agent_file_change_card_drops_discarded_paths(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("notes.txt", "alpha\nbeta\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);

    let panel = cx.read(|cx| h.app.read(cx).agent.clone()).unwrap();
    panel.update(cx, |panel, cx| {
        panel.seed_chat_session_for_test("chat-1");
        panel.push_user_for_test("edit", cx);
    });
    let (reply, answer) = flume::bounded(1);
    h.agent_event(
        AgentPanelEvent::WriteFile(FileWrite {
            path: path.clone(),
            content: "alpha\nBETA\n".into(),
            reply,
        }),
        cx,
    );
    assert_eq!(answer.try_recv().unwrap(), Ok(()));
    panel.update(cx, |panel, cx| panel.flush_file_changes_for_test(cx));
    assert!(!cx.read(|cx| panel.read(cx).file_change_summaries_for_test()).is_empty());

    panel.update(cx, |panel, cx| {
        panel.note_discarded_paths(std::slice::from_ref(&path), cx);
    });
    assert!(
        cx.read(|cx| panel.read(cx).file_change_summaries_for_test()).is_empty(),
        "discard should clear the card rows"
    );
    assert!(
        cx.read(|cx| panel.read(cx).chats()[0].file_changes.is_empty()),
        "persisted cards should drop emptied turns"
    );
}

#[gpui_kit::test]
fn agent_edit_tool_without_write_rpc_shows_file_summary(cx: &mut TestAppContext) {
    use agent_client_protocol::schema::v1::ToolCallStatus;

    let h = Harness::new(cx);
    let path = h.file("readme.md", "hello\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);

    let panel = cx.read(|cx| h.app.read(cx).agent.clone()).unwrap();
    // Pending snapshots baseline before the agent writes on disk.
    panel.update(cx, |panel, cx| {
        panel.note_edit_tool_for_test(&path, ToolCallStatus::Pending, cx);
    });
    std::fs::write(&path, "hello\n(test)\n").unwrap();
    panel.update(cx, |panel, cx| {
        panel.note_edit_tool_for_test(&path, ToolCallStatus::Completed, cx);
        panel.flush_file_changes_for_test(cx);
    });
    let summary = cx.read(|cx| panel.read(cx).file_change_summaries_for_test());
    assert_eq!(summary, vec![("readme.md".into(), 1, 0, false)]);
}

#[gpui_kit::test]
fn agent_shell_style_git_edits_show_file_summary(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let root = h.dir.path().to_path_buf();
    h.file("tracked.txt", "old\n");
    h.file("doomed.txt", "x\n");
    // Shell-style agent edits only show up via git worktree delta.
    assert!(
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root)
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&root)
        .status();
    assert!(
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    h.open_folder(&root, cx);
    h.press("secondary-shift-b", cx);
    let panel = cx.read(|cx| h.app.read(cx).agent.clone()).unwrap();
    panel.update(cx, |panel, _cx| panel.begin_turn_file_tracking_for_test());

    std::fs::write(root.join("tmp-test.txt"), "tmp\n").unwrap();
    std::fs::write(root.join("tracked.txt"), "old\nnew\n").unwrap();
    std::fs::remove_file(root.join("doomed.txt")).unwrap();

    panel.update(cx, |panel, cx| panel.flush_file_changes_for_test(cx));
    let summary = cx.read(|cx| panel.read(cx).file_change_summaries_for_test());
    let names: Vec<_> = summary.iter().map(|(n, _, _, _)| n.as_str()).collect();
    assert!(names.contains(&"tmp-test.txt"), "{summary:?}");
    assert!(names.contains(&"tracked.txt"), "{summary:?}");
    assert!(names.contains(&"doomed.txt"), "{summary:?}");
}

#[gpui_kit::test]
fn agent_at_mention_shows_workspace_files(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.file("alpha.rs", "a\n");
    h.file("beta.rs", "b\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    h.agent_composer("@", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        !labels.is_empty(),
        "mention menu populated on @: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "alpha.rs" || l.ends_with("alpha.rs")),
        "root files listed: {labels:?}"
    );
    assert!(h.has("agent-mention-menu", cx) || h.has("agent-mention-0", cx), "menu in tree");

    h.agent_composer("@alp", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        labels.iter().any(|l| l.contains("alpha")),
        "filtered: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.contains("beta")),
        "filtered out beta: {labels:?}"
    );
}

/// Nested folders must appear on empty `@` and when filtering by the nested name.
#[gpui_kit::test]
fn agent_at_mention_finds_nested_folders(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.folder("src/agent");
    h.file("src/agent/mod.rs", "// agent\n");
    h.file("README.md", "hi\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    h.agent_composer("@", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        labels.iter().any(|l| l == "src" || l.ends_with("/src")),
        "root folder src: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "src/agent" || l.ends_with("/src/agent")),
        "nested folder src/agent on empty @: {labels:?}"
    );

    h.agent_composer("@agent", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        labels.iter().any(|l| l.contains("src/agent") || l.ends_with("agent")),
        "filter 'agent' must hit nested folder, got {labels:?}"
    );

    h.agent_composer("@src/agent", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        labels.iter().any(|l| l.contains("src/agent")),
        "filter 'src/agent' must hit nested folder, got {labels:?}"
    );
}

/// Multi-root workspace: `@royal5g-system` must hit that root (not "No matches").
#[gpui_kit::test]
fn agent_at_mention_matches_multi_root_folder(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let graph = h.folder("royal5g-graph");
    let system = h.folder("royal5g-system");
    std::fs::write(system.join("note.md"), "x\n").unwrap();
    h.open_folder(&graph, cx);
    h.with_app(cx, |app, window, cx| {
        app.add_folders(std::slice::from_ref(&system), window, cx);
    });
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    h.agent_composer("@", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        labels.iter().any(|l| l == "royal5g-system"),
        "empty @ lists workspace roots: {labels:?}"
    );

    h.agent_composer("@royal5g-system", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        labels.iter().any(|l| l == "royal5g-system"),
        "filter must match workspace root, got {labels:?}"
    );

    h.agent_composer("@note", cx);
    let labels = h.agent_mention_labels(cx);
    assert!(
        labels.iter().any(|l| l.contains("note")),
        "search inside non-primary root must work, got {labels:?}"
    );
}

#[gpui_kit::test]
fn reordering_roots_changes_the_agents_primary_folder(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let first = h.folder("first");
    let second = h.folder("second");
    h.open_folder(&first, cx);
    h.with_app(cx, |app, window, cx| {
        app.add_folders(std::slice::from_ref(&second), window, cx);
    });
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    let agent_roots = |h: &Harness, cx: &mut TestAppContext| -> Vec<PathBuf> {
        cx.read(|cx| {
            let app = h.app.read(cx);
            app.agent.as_ref().unwrap().read(cx).roots_for_test().to_vec()
        })
    };
    assert_eq!(agent_roots(&h, cx), [first.clone(), second.clone()]);

    // Drag `second` onto `first` → it becomes the primary folder.
    let action = crate::ui::file_tree::ReorderRoot { folder: second.clone(), before: Some(first.clone()) };
    h.with_app(cx, |app, window, cx| app.on_reorder_root(&action, window, cx));
    assert_eq!(agent_roots(&h, cx), [second.clone(), first.clone()]);
    let ws = cx.read(|cx| h.app.read(cx).workspace.folders().to_vec());
    assert_eq!(ws, [second, first]);
}

#[gpui_kit::test]
fn agent_at_mention_is_visual_token_not_path_or_attach_chip(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.file("note.rs", "fn main() {}\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    h.agent_composer("@note", cx);
    h.with_app(cx, |app, window, cx| {
        let panel = app.agent.clone().expect("agent");
        panel.update(cx, |panel, cx| panel.confirm_mention_for_test(window, cx));
    });
    let (attach, mentions) = cx.read(|cx| {
        let panel = h.app.read(cx).agent.as_ref().unwrap().read(cx);
        (
            panel.attachment_labels(),
            panel.prompt_mention_names_for_test(cx),
        )
    });
    assert!(
        attach.is_empty(),
        "paperclip chip row stays empty, got {attach:?}"
    );
    assert_eq!(
        mentions, ["note.rs"],
        "mention is an inline token inside the prompt"
    );
}

#[gpui_kit::test]
fn agent_attach_path_adds_chip_and_picker_button_exists(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let path = h.file("note.rs", "fn main() {}\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    assert!(h.has("agent-attach-file", cx), "disk picker button");
    h.agent_event(AgentPanelEvent::AttachPath(path), cx);
    let labels = cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .attachment_labels()
    });
    assert_eq!(labels, ["note.rs"]);
}

#[gpui_kit::test]
fn agent_ctrl_l_attaches_editor_selection(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let path = h.file("note.rs", "line one\nline two\nline three\n");
    h.open_folder(h.dir.path(), cx);
    h.open(&path, cx);
    // Drive the action (same as ⌘L / Ctrl+L). Kit keystroke dispatch for
    // `secondary-l` has been unreliable on GitHub's Ubuntu runners; the
    // binding + menu still call `on_add_selection_to_agent`.
    h.with_app(cx, |app, window, cx| {
        let state = app
            .tabs
            .active()
            .and_then(|t| t.editor_state())
            .expect("editor")
            .clone();
        // Select "line two\n" (bytes after first newline through second).
        let start = "line one\n".len();
        let end = "line one\nline two\n".len();
        state.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.focus(window, cx);
        });
        app.on_add_selection_to_agent(&super::AddSelectionToAgent, window, cx);
    });
    let labels = cx.read(|cx| {
        let app = h.app.read(cx);
        assert!(app.agent_visible, "agent panel opens");
        app.agent
            .as_ref()
            .unwrap()
            .read(cx)
            .attachment_labels()
    });
    assert_eq!(labels, ["note.rs:2"]);
}

#[gpui_kit::test]
fn agent_attach_image_path_is_image_chip_not_file(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let a = h.dir.path().join("a.png");
    let b = h.dir.path().join("b.png");
    std::fs::write(&a, png).unwrap();
    std::fs::write(&b, png).unwrap();
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    h.agent_event(AgentPanelEvent::AttachPath(a), cx);
    h.agent_event(AgentPanelEvent::AttachPath(b), cx);
    let labels = cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .attachment_labels()
    });
    assert_eq!(labels, ["a.png", "b.png"]);
}

#[gpui_kit::test]
fn agent_ctrl_n_starts_new_chat(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    h.ask("hello", cx);
    assert_eq!(h.transcript(cx), ["you: hello", "agent: echo: hello"]);
    let tabs_before = cx.read(|cx| h.app.read(cx).tabs.len());
    // Composer was focused on panel open; ask() types into it — keep focus.
    h.with_app(cx, |app, window, cx| {
        let panel = app.agent.clone().expect("agent");
        panel.update(cx, |panel, cx| {
            panel.input().update(cx, |input, cx| input.focus(window, cx));
        });
    });
    h.press("secondary-n", cx);
    h.wait_ready(cx);
    assert!(
        h.transcript(cx).is_empty(),
        "Ctrl/Cmd+N in composer must open a new chat"
    );
    assert_eq!(
        cx.read(|cx| h.app.read(cx).tabs.len()),
        tabs_before,
        "must not create an Untitled editor tab"
    );
}

#[gpui_kit::test]
fn agent_config_row_survives_new_chat_open(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    assert!(cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .has_composer_config_for_test()
    }));
    // New chat goes Opening with prior config_options kept — no row collapse.
    h.with_app(cx, |app, window, cx| {
        let panel = app.agent.clone().expect("agent");
        panel.update(cx, |panel, cx| {
            panel.new_chat_for_test(window, cx);
            assert!(
                panel.has_composer_config_for_test(),
                "config row must stay mounted while Opening"
            );
        });
    });
    h.wait_ready(cx);
    assert!(cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .has_composer_config_for_test()
    }));
}

#[gpui_kit::test]
fn agent_shows_context_placeholder_before_prompt(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    // No turn yet — Claude would not have sent usage_update; we seed size.
    h.wait_for(cx, "context placeholder", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                a.read(cx).context_usage_label().as_deref() == Some("— / 200K")
            })
        })
    });
}

#[gpui_kit::test]
fn agent_mode_hides_auto_when_model_too_old(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    // Default fake model is Sonnet 4.5 — Claude Auto needs 4.6+.
    let ids = cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .mode_choice_ids()
    });
    assert!(
        !ids.iter().any(|id| id == "auto"),
        "Auto hidden on Sonnet 4.5: {ids:?}"
    );
    assert!(ids.iter().any(|id| id == "ask"), "{ids:?}");
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.set_model_version_for_test("4.6", cx);
        });
    });
    h.wait_for(cx, "sonnet 4.6 auto shown", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                let a = a.read(cx);
                a.current_model_id().as_deref() == Some("sonnet-4-6")
                    && a.mode_choice_ids().iter().any(|id| id == "auto")
            })
        })
    });
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.set_model_family_for_test("Haiku", cx);
        });
    });
    h.wait_for(cx, "haiku hides auto", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                let a = a.read(cx);
                a.current_model_id().as_deref() == Some("haiku-4-5")
                    && !a.mode_choice_ids().iter().any(|id| id == "auto")
            })
        })
    });
}

#[gpui_kit::test]
fn agent_shows_context_usage_after_prompt(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    h.ask("hello", cx);
    // Fake agent: 10K + 2 msgs × 5K = 20K of 200K after one turn.
    h.wait_for(cx, "context usage", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                a.read(cx).context_usage_label().as_deref() == Some("20K / 200K")
            })
        })
    });
}

#[gpui_kit::test]
fn agent_chats_belong_to_their_folder_and_survive_switching(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let a = h.folder("a");
    let b = h.folder("b");

    h.open_folder(&a, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    h.ask("hello", cx);
    assert_eq!(h.transcript(cx), ["you: hello", "agent: echo: hello"]);
    assert_eq!(h.saved_chats(&a, cx), (vec!["s1".into()], Some("s1".into())));

    // Folder B gets its own, empty chat.
    h.open_folder(&b, cx);
    assert!(!h.agent_visible(cx), "B has its own (hidden) panel state");
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    assert!(h.transcript(cx).is_empty());
    assert_eq!(h.saved_chats(&b, cx).0, Vec::<String>::new());

    // Back to A: same transcript, resumed without a replay.
    h.open_folder(&a, cx);
    h.wait_ready(cx);
    assert_eq!(h.transcript(cx), ["you: hello", "agent: echo: hello"]);
    let calls = agent.calls();
    assert!(calls.contains(&"resume s1".to_string()), "{calls:?}");
    assert!(!calls.contains(&"load s1".to_string()), "{calls:?}");

    // A fresh app (restart) loads the chat and replays its history.
    let config = cx.read(|cx| h.app.read(cx).config.clone());
    let h2 = Harness::with_config(config, cx);
    h2.open_folder(&a, cx);
    h2.wait_ready(cx);
    assert_eq!(h2.transcript(cx), ["you: hello", "agent: echo: hello"]);
    assert!(agent.calls().contains(&"load s1".to_string()));
}

#[gpui_kit::test]
fn agent_auto_titles_after_first_turn(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let folder = h.folder("proj");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    h.ask("please fix the flaky test", cx);
    h.wait_for(cx, "auto title replaces first-line placeholder", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                a.read(cx).active_chat_title().as_deref() == Some("Generated Title")
            })
        })
    });
    // Manual rename cancels further auto-title attempts.
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.rename_chat("s1", "Keep me", cx);
        });
    });
    h.ask("follow up", cx);
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).active_chat_title()),
        Some("Keep me".into()),
        "user rename must not be overwritten on later turns"
    );
}

#[gpui_kit::test]
fn agent_rename_and_mode_model_switch(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let folder = h.folder("proj");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    h.ask("hello", cx);
    // First turn auto-titles (Claude sends no session_info_update title).
    h.wait_for(cx, "auto chat title", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                a.read(cx).active_chat_title().as_deref() == Some("Generated Title")
            })
        })
    });

    // Rename is local (persisted with the chat index).
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.rename_chat("s1", "My notes", cx);
            assert_eq!(panel.active_chat_title().as_deref(), Some("My notes"));
        });
    });
    let titles: Vec<String> = cx.read(|cx| {
        h.app
            .read(cx)
            .config
            .session(&folder.to_string_lossy())
            .map(|s| s.agent_chats.iter().map(|c| c.title.clone()).collect())
            .unwrap_or_default()
    });
    assert_eq!(titles, ["My notes"]);

    // Fake agent: Auto, Sonnet 4.5 (+ 4.6 / 1M / Opus), Fast Off.
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).current_mode_id()),
        Some("auto".into())
    );
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).current_model_id()),
        Some("sonnet-4-5".into())
    );
    assert_eq!(
        cx.read(|cx| {
            h.app
                .read(cx)
                .agent
                .as_ref()
                .unwrap()
                .read(cx)
                .current_model_family()
        }),
        Some("Sonnet".into())
    );
    assert_eq!(
        cx.read(|cx| {
            h.app
                .read(cx)
                .agent
                .as_ref()
                .unwrap()
                .read(cx)
                .current_model_version_label()
        }),
        Some("4.5".into())
    );
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).fast_mode_on()),
        Some(false)
    );
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.set_mode_for_test("code", cx);
            panel.set_model_version_for_test("4.6", cx);
        });
    });
    h.wait_for(cx, "mode/version apply", |_cx| {
        let calls = agent.calls();
        calls.iter().any(|c| c == "set_mode code")
            && calls.iter().any(|c| c == "set_config model=sonnet-4-6")
    });
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).current_mode_id()),
        Some("code".into())
    );
    h.wait_for(cx, "sonnet 4.6 selected", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                let a = a.read(cx);
                a.current_model_id().as_deref() == Some("sonnet-4-6")
                    && a.current_model_version_label().as_deref() == Some("4.6")
                    && a.current_model_context_label().as_deref() == Some("200K")
            })
        })
    });
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.set_model_context_for_test("1M", cx);
            panel.set_fast_mode_for_test(true, cx);
        });
    });
    h.wait_for(cx, "context 1M + fast on", |_cx| {
        let calls = agent.calls();
        calls.iter().any(|c| c == "set_config model=sonnet-4-6[1m]")
            && calls.iter().any(|c| c == "set_config fast_mode=on")
    });
    h.wait_for(cx, "1M + fast applied", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                let a = a.read(cx);
                a.current_model_id().as_deref() == Some("sonnet-4-6[1m]")
                    && a.current_model_context_label().as_deref() == Some("1M")
                    && a.fast_mode_on() == Some(true)
            })
        })
    });
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.set_model_family_for_test("Opus", cx);
        });
    });
    h.wait_for(cx, "opus family keeps 1M", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                let a = a.read(cx);
                a.current_model_id().as_deref() == Some("opus-5[1m]")
                    && a.current_model_family().as_deref() == Some("Opus")
                    && a.current_model_context_label().as_deref() == Some("1M")
            })
        })
    });

    // AI title for the open chat (silent turn).
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            assert!(panel.request_chat_title("s1".into(), cx));
        });
    });
    h.wait_for(cx, "AI chat title", |cx| {
        cx.read(|cx| {
            h.app
                .read(cx)
                .agent
                .as_ref()
                .is_some_and(|a| a.read(cx).active_chat_title().as_deref() == Some("Generated Title"))
        })
    });

    // Second chat, then Generate for the first via cached transcript.
    let first_id = cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .active_chat_id()
            .expect("active chat after title")
    });
    h.with_app(cx, |app, window, cx| {
        let panel = app.agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.new_chat_for_test(window, cx);
        });
    });
    h.wait_ready(cx);
    // Picks are saved per folder (and globally) and re-applied to the new chat.
    h.wait_for(cx, "new chat keeps mode/model/fast", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                let a = a.read(cx);
                a.current_mode_id().as_deref() == Some("code")
                    && a.current_model_id().as_deref() == Some("opus-5[1m]")
                    && a.fast_mode_on() == Some(true)
            })
        })
    });
    let (saved, global, live) = cx.read(|cx| {
        let app = h.app.read(cx);
        (
            app.config
                .session(&folder.to_string_lossy())
                .map(|s| s.agent_prefs.clone()),
            app.config.agent_prefs.clone(),
            app.agent.as_ref().unwrap().read(cx).prefs().clone(),
        )
    });
    assert_eq!(live.mode.as_deref(), Some("code"));
    assert_eq!(saved.as_ref(), Some(&live));
    assert_eq!(global, live);
    h.ask("other", cx);
    assert_ne!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).active_chat_id()),
        Some(first_id.clone())
    );
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            assert!(
                panel.request_chat_title(first_id.clone(), cx),
                "Generate should work for a non-active chat via cache"
            );
        });
    });
    h.wait_for(cx, "AI title for background chat", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| {
                a.read(cx)
                    .chat_title(&first_id)
                    .as_deref()
                    == Some("Generated Title")
            })
        })
    });
}

#[gpui_kit::test]
fn quitting_while_the_agent_runs_asks_first(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let folder = h.folder("q");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    let release = agent.arm_hold();
    h.input("hold", cx);
    h.press("enter", cx);
    h.wait_for(cx, "turn running", |cx| {
        cx.read(|cx| {
            h.app.read(cx).agent.as_ref().is_some_and(|a| a.read(cx).is_turn_running())
        })
    });
    h.with_app(cx, |app, window, cx| {
        assert!(!app.confirm_quit(window, cx), "must ask, not quit");
    });
    assert!(h.has("dialog-secondary", cx), "Quit Anyway offered");
    assert!(h.has("dialog-primary", cx), "Keep Working offered");
    h.click("dialog-primary", cx);
    assert!(!h.has("dialog-secondary", cx));
    agent.release_hold(release);
}

#[gpui_kit::test]
fn agent_queues_follow_up_while_a_turn_runs(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let folder = h.folder("q");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    let release = agent.arm_hold();
    h.input("hold", cx);
    h.press("enter", cx);
    h.wait_for(cx, "first turn running", |cx| {
        cx.read(|cx| {
            h.app
                .read(cx)
                .agent
                .as_ref()
                .is_some_and(|a| a.read(cx).is_turn_running())
        })
    });
    h.input("next", cx);
    h.press("enter", cx);
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).outbound_queue_len()),
        1,
        "follow-up should be queued while hold runs"
    );
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).outbound_previews()),
        ["next"]
    );
    let t = h.transcript(cx);
    assert!(t.iter().any(|l| l == "you: hold"), "{t:?}");
    assert!(
        !t.iter().any(|l| l == "you: next"),
        "queued bubble waits until drain: {t:?}"
    );

    // Queue a second follow-up, then drop the first from the strip.
    h.input("skip-me", cx);
    h.press("enter", cx);
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).outbound_previews()),
        ["next", "skip-me"]
    );
    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| panel.remove_outbound_for_test(1, cx));
    });
    assert_eq!(
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).outbound_previews()),
        ["next"]
    );

    agent.release_hold(release);
    // Wall-clock: the fake agent blocks a real thread on hold; GPUI's test
    // clock does not advance that.
    for _ in 0..200 {
        cx.run_until_parked();
        let lines = h.transcript(cx);
        if lines.iter().any(|l| l == "agent: echo: next") {
            assert_eq!(
                lines,
                [
                    "you: hold",
                    "agent: echo: hold",
                    "you: next",
                    "agent: echo: next",
                ],
                "{lines:?}"
            );
            assert_eq!(
                cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).outbound_queue_len()),
                0
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor().advance_clock(std::time::Duration::from_millis(10));
    }
    panic!(
        "queued follow-up never drained; transcript={:?} queue={}",
        h.transcript(cx),
        cx.read(|cx| h.app.read(cx).agent.as_ref().unwrap().read(cx).outbound_queue_len()),
    );
}

#[gpui_kit::test]
fn switching_away_mid_turn_lets_it_finish_then_stops_the_agent(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let a = h.folder("a");
    let b = h.folder("b");
    h.open_folder(&a, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    let panel_a = cx.read(|cx| h.app.read(cx).agent.clone().unwrap());

    // "shout" makes the fake agent read a file first: the turn is still
    // running when we leave. (The read fails — `/w` does not exist — and the
    // agent ends the turn with that error, in the background.)
    h.input("shout", cx);
    h.press("enter", cx);
    h.open_folder(&b, cx);
    let suspended = panel_a.clone();
    h.wait_for(cx, "background turn to end", move |cx| {
        cx.read(|cx| suspended.read(cx).is_suspended())
    });
    let transcript = cx.read(|cx| panel_a.read(cx).transcript(cx));
    assert_eq!(transcript.first().map(String::as_str), Some("you: shout"));
}

#[gpui_kit::test]
fn agent_answers_pending_permission(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let folder = h.folder("proj");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    let reply = cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| panel.inject_pending_permission_for_test(cx))
    });
    assert!(cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .has_pending_permission_for_test()
    }));
    assert!(
        h.has("agent-perm-t-perm-allow", cx),
        "Allow permission button is in the tree"
    );

    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            panel.answer_permission_for_test("allow", cx);
        });
    });
    assert!(!cx.read(|cx| {
        h.app
            .read(cx)
            .agent
            .as_ref()
            .unwrap()
            .read(cx)
            .has_pending_permission_for_test()
    }));
    let answered = reply.try_recv().expect("permission reply");
    assert_eq!(
        answered.map(|id| id.to_string()),
        Some("allow".into()),
        "panel forwarded Allow to the ACP reply channel"
    );
}

#[gpui_kit::test]
fn agent_deletes_chat(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let folder = h.folder("proj");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);
    h.ask("hello", cx);
    assert_eq!(h.saved_chats(&folder, cx).0, vec!["s1".to_string()]);

    h.with_app(cx, |app, window, cx| {
        let panel = app.agent.clone().expect("agent");
        panel.update(cx, |panel, cx| {
            panel.delete_chat_for_test("s1", window, cx);
        });
    });

    let ids = h.saved_chats(&folder, cx).0;
    assert!(!ids.contains(&"s1".to_string()), "s1 removed from index: {ids:?}");
    let calls = agent.calls();
    assert!(
        calls.iter().any(|c| c == "delete s1"),
        "fake agent got delete: {calls:?}"
    );
}

#[gpui_kit::test]
fn agent_install_and_login_screens(cx: &mut TestAppContext) {
    // No FakeAgent: we only assert empty-state chrome, never npm / auth.
    let h = Harness::new(cx);
    let folder = h.folder("proj");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);

    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| panel.force_not_installed_for_test(cx));
    });
    assert!(h.has("agent-install", cx), "Install button");

    cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| panel.force_login_required_for_test(cx));
    });
    assert!(h.has("agent-retry", cx), "Retry after login");
    assert!(h.has("agent-login-0", cx), "login method button");
}

#[gpui_kit::test]
fn tree_create_file_and_folder_land_on_disk(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let root = h.dir.path().to_path_buf();
    h.open_folder(&root, cx);

    let file = root.join("fresh.txt");
    let r = h.with_app(cx, |app, w, cx| app.create_file_in(&root, "fresh.txt", w, cx));
    assert!(r.is_ok(), "{r:?}");
    assert!(file.is_file());
    assert!(h.tab(&file, cx).is_some(), "new file opens as a tab");

    let dir = root.join("nested");
    let r = h.with_app(cx, |app, w, cx| app.create_folder_in(&root, "nested", w, cx));
    assert!(r.is_ok(), "{r:?}");
    assert!(dir.is_dir());

    let r = h.with_app(cx, |app, w, cx| app.create_file_in(&root, "fresh.txt", w, cx));
    assert!(r.is_err(), "duplicate name refused");
}

#[gpui_kit::test]
fn tree_rename_remaps_open_tab(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("old.txt", "body\n");
    h.open_folder(h.dir.path(), cx);
    h.open(&path, cx);
    assert_eq!(h.tab(&path, cx).map(|(_, t)| t), Some("body\n".into()));

    let renamed = h.dir.path().join("new.txt");
    let r = h.with_app(cx, |app, w, cx| app.rename_path(&path, "new.txt", w, cx));
    assert!(r.is_ok(), "{r:?}");
    assert!(!path.exists() && renamed.exists());
    assert!(h.tab(&renamed, cx).is_some());
    assert!(h.tab(&path, cx).is_none());
    assert_eq!(
        h.tab(&renamed, cx).map(|(_, t)| t),
        Some("body\n".into()),
        "buffer text follows the rename"
    );
}

impl Harness {
    fn terminal_visible(&self, cx: &mut TestAppContext) -> bool {
        cx.read(|cx| self.app.read(cx).terminal.read(cx).is_visible())
    }

    fn terminal_session_count(&self, cx: &mut TestAppContext) -> usize {
        cx.read(|cx| self.app.read(cx).terminal.read(cx).session_cwds().len())
    }
}

#[gpui_kit::test]
fn terminal_toggle_spawns_session_and_hides(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    assert!(!h.terminal_visible(cx));
    assert_eq!(h.terminal_session_count(cx), 0);

    // Call the same handlers the keybinding / menu use (test window focus
    // often misses `ctrl-\`` chords).
    h.with_app(cx, |app, w, cx| {
        app.on_toggle_terminal(&crate::app::ToggleTerminal, w, cx);
    });
    assert!(h.terminal_visible(cx), "toggle opens the panel");
    assert_eq!(h.terminal_session_count(cx), 1, "first toggle creates a shell");

    h.with_app(cx, |app, w, cx| {
        app.on_toggle_terminal(&crate::app::ToggleTerminal, w, cx);
    });
    assert!(!h.terminal_visible(cx), "toggle hides without dropping the session");
    assert_eq!(h.terminal_session_count(cx), 1);

    h.with_app(cx, |app, w, cx| {
        app.on_toggle_terminal(&crate::app::ToggleTerminal, w, cx);
    });
    assert!(h.terminal_visible(cx), "show again reuses the session");
    assert_eq!(h.terminal_session_count(cx), 1);
}

#[gpui_kit::test]
fn terminal_new_adds_another_tab(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    h.with_app(cx, |app, w, cx| {
        app.on_toggle_terminal(&crate::app::ToggleTerminal, w, cx);
    });
    assert_eq!(h.terminal_session_count(cx), 1);

    h.with_app(cx, |app, w, cx| {
        app.on_new_terminal(&crate::app::NewTerminal, w, cx);
    });
    assert!(h.terminal_visible(cx));
    assert_eq!(h.terminal_session_count(cx), 2, "New Terminal adds a tab");
}

#[gpui_kit::test]
fn save_as_writes_new_path_and_leaves_original(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    let path = h.file("a.txt", "hello");
    h.open(&path, cx);
    h.input("X", cx);
    assert_eq!(h.tab(&path, cx), Some((true, "Xhello".into())));

    let as_path = h.dir.path().join("a-copy.txt");
    let r = h.with_app(cx, |app, w, cx| app.save_as_to(&path, as_path.clone(), w, cx));
    assert!(r.is_ok(), "{r:?}");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello", "original untouched");
    assert_eq!(std::fs::read_to_string(&as_path).unwrap(), "Xhello");
    assert!(h.tab(&path, cx).is_none(), "tab left the old path");
    assert_eq!(h.tab(&as_path, cx), Some((false, "Xhello".into())));
}

#[gpui_kit::test]
fn dirty_buffer_text_restores_with_the_folder_session(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let a = h.folder("a");
    let b = h.folder("b");
    let path = a.join("note.txt");
    std::fs::write(&path, "base").unwrap();

    h.open_folder(&a, cx);
    h.open(&path, cx);
    h.input("EDIT-", cx);
    assert_eq!(h.tab(&path, cx), Some((true, "EDIT-base".into())));

    // Switch away: Don't Save still persist_session (with unsaved_text) first.
    h.open_folder(&b, cx);
    assert!(h.has("dialog-secondary", cx), "unsaved-changes before switch");
    h.click("dialog-secondary", cx);
    assert!(h.tab(&path, cx).is_none());

    h.open_folder(&a, cx);
    assert_eq!(
        h.tab(&path, cx),
        Some((true, "EDIT-base".into())),
        "unsaved text + dirty mark come back with the folder"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "base", "disk unchanged");
}

#[gpui_kit::test]
fn quit_asks_to_save_untitled_multi_root_workspace(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let first = h.folder("first");
    let second = h.folder("second");
    h.open_folder(&first, cx);
    h.with_app(cx, |app, window, cx| {
        app.add_folders(std::slice::from_ref(&second), window, cx);
        assert!(
            app.workspace.is_untitled_multi_root(),
            "two roots, no workspace file"
        );
        assert!(
            !app.confirm_quit(window, cx),
            "must show a dialog, not quit immediately"
        );
    });
    assert!(
        h.has("dialog-secondary", cx),
        "Save workspace? dialog (Don't Save / Save…)"
    );
    // Cancel — don't drive quit in the harness.
    h.click("dialog-cancel", cx);
    assert!(!h.has("dialog-secondary", cx));
}

#[gpui_kit::test]
fn disk_reload_keeps_selection(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "abcdefghij");
    h.open(&path, cx);
    h.with_app(cx, |app, _, cx| {
        let state = app
            .tabs
            .iter()
            .find(|t| t.path == path)
            .and_then(|t| t.editor_state())
            .expect("editor")
            .clone();
        state.update(cx, |state, cx| state.set_selected_range(3..7, cx));
    });

    std::fs::write(&path, "abcdefghijXXX").unwrap();
    h.with_app(cx, |app, window, cx| {
        app.reload_open_file_from_disk(&path, window, cx);
    });

    let (text, range) = h.with_app(cx, |app, _, cx| {
        let tab = app.tabs.iter().find(|t| t.path == path).expect("tab");
        let state = tab.editor_state().expect("editor");
        let range = state.read(cx).selected_range();
        (tab.editor_state().unwrap().read(cx).value().to_string(), range)
    });
    assert_eq!(text, "abcdefghijXXX");
    assert_eq!(range, 3..7, "selection survives clean reload");
}

#[gpui_kit::test]
fn deleted_clean_tab_closes_dirty_stays(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let clean = h.file("clean.txt", "ok");
    let dirty = h.file("dirty.txt", "ok");
    h.open(&clean, cx);
    h.open(&dirty, cx);
    h.input("X", cx);
    assert_eq!(h.tab(&dirty, cx).map(|t| t.0), Some(true));

    std::fs::remove_file(&clean).unwrap();
    std::fs::remove_file(&dirty).unwrap();
    h.with_app(cx, |app, window, cx| {
        use crate::watch::FsChange;
        app.apply_fs_changes(
            vec![
                FsChange {
                    path: clean.clone(),
                    structural: true,
                },
                FsChange {
                    path: dirty.clone(),
                    structural: true,
                },
            ],
            window,
            cx,
        );
    });

    assert!(h.tab(&clean, cx).is_none(), "clean deleted tab closes");
    assert_eq!(
        h.tab(&dirty, cx),
        Some((true, "Xok".into())),
        "dirty deleted tab keeps buffer"
    );
}

#[gpui_kit::test]
fn reconcile_reloads_clean_tab_after_external_write(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let path = h.file("a.txt", "old");
    h.open(&path, cx);
    std::fs::write(&path, "new").unwrap();
    h.with_app(cx, |app, window, cx| {
        app.reconcile_open_tabs_with_disk(window, cx);
    });
    assert_eq!(h.tab(&path, cx), Some((false, "new".into())));
}

#[gpui_kit::test]
fn workspace_settings_overlay_font_tab_and_format(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let root = h.folder("proj");
    let ws = h.dir.path().join(format!("proj.{}", crate::workspace::WORKSPACE_EXTENSION));
    std::fs::write(
        &ws,
        r#"{
  "folders": [{ "path": "proj" }],
  "settings": {
    "editor.fontSize": 20,
    "tabSize": 4,
    "editor.formatOnSave": false
  }
}"#,
    )
    .unwrap();
    // Folder path in the file is relative to the workspace file's directory.
    assert_eq!(root, h.dir.path().join("proj"));

    h.with_app(cx, |app, window, cx| {
        app.open_workspace_file(&ws, window, cx);
        let eff = app.effective_config();
        assert_eq!(eff.font_size, 20.0);
        assert_eq!(eff.tab_size, 4);
        assert!(!eff.lsp.format_on_save);
        assert_eq!(app.config.font_size, AppConfig::default().font_size, "user config untouched");
    });
}

#[gpui_kit::test]
fn commit_suggest_runs_in_parallel_across_repos(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let folder = h.folder("proj");
    h.open_folder(&folder, cx);
    h.press("secondary-shift-b", cx);
    h.wait_ready(cx);

    let (first, second, same_again, ready_after) = cx.update(|cx| {
        let panel = h.app.read(cx).agent.clone().unwrap();
        panel.update(cx, |panel, cx| {
            let a = panel.request_commit_message("/repo-a".into(), "a".into(), cx);
            let ready = panel.can_suggest_commit();
            let b = panel.request_commit_message("/repo-b".into(), "b".into(), cx);
            let dup = panel.request_commit_message("/repo-a".into(), "a".into(), cx);
            (a, b, dup, ready)
        })
    });
    assert!(first, "first repo starts");
    assert!(ready_after, "Suggest stays available in other repos");
    assert!(second, "second repo starts while the first is running");
    assert!(!same_again, "the same repo cannot double-start");
}

#[gpui_kit::test]
fn git_repo_sections_collapse_independently(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let a = PathBuf::from("/repo-a");
    let b = PathBuf::from("/repo-b");
    let (a_collapsed, b_collapsed, a_again) = cx.update(|cx| {
        let git = h.app.read(cx).git.clone();
        git.update(cx, |git, cx| {
            git.toggle_repo(&a, cx);
            let first = (git.is_repo_collapsed(&a), git.is_repo_collapsed(&b));
            git.toggle_repo(&a, cx);
            (first.0, first.1, git.is_repo_collapsed(&a))
        })
    });
    assert!(a_collapsed, "toggled repo folds");
    assert!(!b_collapsed, "other repo is untouched");
    assert!(!a_again, "second toggle expands again");
}

fn fake_release(version: &str) -> crate::update::Release {
    crate::update::Release {
        tag: format!("v{version}"),
        version: version.into(),
        notes: "- something new".into(),
        page_url: "https://example.invalid/release".into(),
        assets: Vec::new(),
    }
}

#[gpui_kit::test]
fn update_dialog_skip_remembers_version(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    h.with_app(cx, |app, window, cx| {
        app.show_update_available(
            fake_release("9.9.9"),
            crate::update::InstallKind::Manual,
            false,
            window,
            cx,
        );
    });
    assert!(h.has("update-primary", cx), "availability dialog is open");
    h.click("update-skip", cx);
    assert!(!h.has("update-primary", cx), "dialog closes");
    let skipped = cx.read(|cx| h.app.read(cx).config.updates.skipped_version.clone());
    assert_eq!(skipped.as_deref(), Some("9.9.9"));
}

#[gpui_kit::test]
fn update_dialog_later_closes_without_skipping(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    h.with_app(cx, |app, window, cx| {
        app.show_update_available(
            fake_release("9.9.9"),
            crate::update::InstallKind::Manual,
            false,
            window,
            cx,
        );
    });
    h.click("update-later", cx);
    assert!(!h.has("update-primary", cx));
    let skipped = cx.read(|cx| h.app.read(cx).config.updates.skipped_version.clone());
    assert_eq!(skipped, None);
}

#[gpui_kit::test]
fn update_check_result_decides_whether_to_prompt(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    // Same version: silent on startup checks, no dialog, check time recorded.
    h.with_app(cx, |app, window, cx| {
        app.finish_update_check(Ok(fake_release(crate::update::current_version())), false, window, cx);
    });
    assert!(!h.has("update-primary", cx));
    assert!(cx.read(|cx| h.app.read(cx).config.updates.last_check.is_some()));

    // A skipped version stays quiet unless the user asked.
    h.with_app(cx, |app, window, cx| {
        app.config.updates.skipped_version = Some("9.9.9".into());
        app.finish_update_check(Ok(fake_release("9.9.9")), false, window, cx);
    });
    assert!(!h.has("update-primary", cx), "skipped version is not offered");

    // A newer, unskipped version prompts.
    h.with_app(cx, |app, window, cx| {
        app.finish_update_check(Ok(fake_release("9.9.10")), false, window, cx);
    });
    assert!(h.has("update-primary", cx));
}

#[gpui_kit::test]
fn update_restart_dialog_goes_through_unsaved_prompt(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let root = h.dir.path().to_path_buf();
    h.open_folder(&root, cx);
    let file = h.file("a.txt", "a");
    h.open(&file, cx);
    h.input("!", cx);

    h.with_app(cx, |app, window, cx| {
        app.show_update_restart(PathBuf::from("/nonexistent/isengard"), window, cx);
    });
    assert!(h.has("update-restart", cx));
    h.click("update-restart", cx);
    // Dirty tab: the quit prompt appears and the app is still running.
    assert!(h.has("dialog-secondary", cx), "unsaved-changes prompt");
    assert!(cx.read(|cx| h.app.read(cx).relaunch_exe.is_some()));
}

#[gpui_kit::test]
fn update_install_command_dialog_copies_and_types(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let root = h.dir.path().to_path_buf();
    h.open_folder(&root, cx);
    let package = PathBuf::from("/tmp/isengard-9.9.9-1.x86_64.rpm");
    let expected = crate::update::install_command("dnf", &package);

    h.with_app(cx, |app, window, cx| {
        app.show_update_install_command(package.clone(), "dnf", window, cx);
    });
    assert!(h.has("update-terminal", cx));
    h.click("update-copy", cx);
    let copied = cx.read(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some(expected.as_str()));
    assert!(!h.has("update-terminal", cx), "dialog closes after copy");

    h.with_app(cx, |app, window, cx| {
        app.show_update_install_command(package.clone(), "dnf", window, cx);
    });
    h.click("update-terminal", cx);
    assert!(
        cx.read(|cx| h.app.read(cx).terminal.read(cx).has_sessions()),
        "terminal tab opened with the command typed"
    );
}

#[gpui_kit::test]
fn update_install_now_asks_for_the_password(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let package = PathBuf::from("/tmp/isengard-9.9.9-1.x86_64.rpm");
    h.with_app(cx, |app, window, cx| {
        app.show_update_install_command(package.clone(), "dnf", window, cx);
    });
    h.click("update-install", cx);
    assert!(h.has("update-password-ok", cx), "password dialog opens");
    assert!(!h.has("update-install", cx));
    h.click("update-password-cancel", cx);
    assert!(!h.has("update-password-ok", cx));
}

#[gpui_kit::test]
fn update_password_enter_starts_the_install(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let package = PathBuf::from("/tmp/isengard-9.9.9-1.x86_64.rpm");
    h.with_app(cx, |app, window, cx| {
        app.show_update_password(package.clone(), "dnf", None, window, cx);
    });
    assert!(h.has("update-password-ok", cx), "password dialog opens");
    // Busy, so the install itself is a no-op: this must not run sudo.
    h.with_app(cx, |app, _, _| app.update_busy = true);
    h.input("hunter2", cx);
    h.press("enter", cx);
    assert!(!h.has("update-password-ok", cx), "dialog closes on Enter");
}

#[gpui_kit::test]
fn check_for_updates_menu_action_reaches_the_handler(cx: &mut TestAppContext) {
    use gpui_kit::Action as _;
    let h = Harness::new(cx);
    *crate::update::TEST_LATEST.lock().unwrap() = Some(fake_release("9.9.9"));
    // Same path as Isengard > Check for Updates…: a dispatched action.
    cx.update_window(h.window.into(), |_, window, cx| {
        window.dispatch_action(super::CheckForUpdates.boxed_clone(), cx);
    })
    .expect("window alive");
    h.wait_for(cx, "the update dialog", |cx| h.has("update-primary", cx));
    *crate::update::TEST_LATEST.lock().unwrap() = None;
}

#[gpui_kit::test]
fn agent_usage_bars_survive_rate_limiting(cx: &mut TestAppContext) {
    use crate::agent::limits::{FetchError, PlanLimits, Window};
    let h = Harness::new(cx);
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-shift-b", cx);
    let panel = cx.read(|cx| h.app.read(cx).agent.clone()).unwrap();
    let state = |cx: &mut TestAppContext| cx.read(|cx| panel.read(cx).plan_limits_state_for_test());
    let limited = || FetchError {
        message: "usage request failed: 429".into(),
        rate_limited: true,
    };
    let week = |pct: f32| PlanLimits {
        session: None,
        week: Some(Window {
            percent: pct,
            resets_at: None,
        }),
        week_opus: None,
        week_sonnet: None,
        breakdown: Vec::new(),
        subscription: None,
    };

    // Fresh start while rate limited: nothing to show, but we know why.
    panel.update(cx, |p, cx| p.apply_limits_fetch_for_test(Err(limited()), cx));
    assert_eq!(state(cx), (false, false, true));

    // Numbers arrive.
    panel.update(cx, |p, cx| p.apply_limits_fetch_for_test(Ok(week(25.0)), cx));
    assert_eq!(state(cx), (true, false, false));

    // Rate limited again: the bar stays, dimmed.
    panel.update(cx, |p, cx| p.apply_limits_fetch_for_test(Err(limited()), cx));
    assert_eq!(state(cx), (true, true, true));

    // An empty 200 is not a recovery.
    panel.update(cx, |p, cx| p.apply_limits_fetch_for_test(Ok(week(25.0)), cx));
    assert_eq!(state(cx), (true, false, false));
    panel.update(cx, |p, cx| p.apply_limits_fetch_for_test(Err("boom".to_string().into()), cx));
    assert_eq!(state(cx), (true, true, false));
    panel.update(cx, |p, cx| {
        let empty = PlanLimits { week: None, ..week(0.0) };
        p.apply_limits_fetch_for_test(Ok(empty), cx);
    });
    assert_eq!(state(cx), (true, true, false), "empty payload keeps stale flag");
}

/// Kit's window `Root` binds Tab to focus traversal, which used to steal it
/// from the shell (no completion). The more specific `Terminal` context must
/// own it. (Pressing Tab for real would make the live shell answer off-thread,
/// which the test scheduler rejects, so assert on the keymap.)
#[gpui_kit::test]
fn tab_in_terminal_is_bound_to_the_shell(cx: &mut TestAppContext) {
    let h = Harness::new(cx);
    let ctx = gpui_kit::KeyContext::parse("Terminal").expect("context");
    let keys = cx
        .update_window(h.window.into(), |_, window, _| {
            let first = |a: &dyn gpui_kit::Action| {
                window
                    .bindings_for_action_in_context(a, ctx.clone())
                    .last()
                    .map(|b| b.keystrokes()[0].unparse())
            };
            (
                first(&crate::terminal::view::SendTab),
                first(&crate::terminal::view::SendBackTab),
            )
        })
        .expect("window alive");
    assert_eq!(keys, (Some("tab".into()), Some("shift-tab".into())));
}
