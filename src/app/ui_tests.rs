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

    h.press("secondary-alt-b", cx);
    assert!(h.agent_visible(cx));
    assert!(h.has("agent-hide", cx));

    h.open_folder(&b, cx);
    assert!(!h.agent_visible(cx), "a new folder starts hidden");
    assert!(!h.has("agent-hide", cx));

    h.open_folder(&a, cx);
    assert!(h.agent_visible(cx), "restored with the folder's session");

    h.press("secondary-alt-b", cx);
    assert!(!h.agent_visible(cx));
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
fn agent_at_mention_shows_workspace_files(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    h.file("alpha.rs", "a\n");
    h.file("beta.rs", "b\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-alt-b", cx);
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
    h.press("secondary-alt-b", cx);
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
        app.add_folders(&[system.clone()], window, cx);
    });
    h.press("secondary-alt-b", cx);
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
fn agent_attach_path_adds_chip_and_picker_button_exists(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let path = h.file("note.rs", "fn main() {}\n");
    h.open_folder(h.dir.path(), cx);
    h.press("secondary-alt-b", cx);
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
    h.press("secondary-alt-b", cx);
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
fn agent_chats_belong_to_their_folder_and_survive_switching(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let a = h.folder("a");
    let b = h.folder("b");

    h.open_folder(&a, cx);
    h.press("secondary-alt-b", cx);
    h.wait_ready(cx);
    h.ask("hello", cx);
    assert_eq!(h.transcript(cx), ["you: hello", "agent: echo: hello"]);
    assert_eq!(h.saved_chats(&a, cx), (vec!["s1".into()], Some("s1".into())));

    // Folder B gets its own, empty chat.
    h.open_folder(&b, cx);
    assert!(!h.agent_visible(cx), "B has its own (hidden) panel state");
    h.press("secondary-alt-b", cx);
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
fn switching_away_mid_turn_lets_it_finish_then_stops_the_agent(cx: &mut TestAppContext) {
    let agent = FakeAgent::new();
    crate::acp::testing::install(&agent);
    let h = Harness::new(cx);
    let a = h.folder("a");
    let b = h.folder("b");
    h.open_folder(&a, cx);
    h.press("secondary-alt-b", cx);
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
