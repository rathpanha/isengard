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
        std::fs::write(&path, text).expect("write fixture");
        path
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
        self.with_app(cx, |app, window, cx| app.handle_agent_event(&event, window, cx));
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
