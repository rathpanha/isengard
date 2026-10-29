//! First-run Bun setup. The agent adapter, `bun fmt` and the npm-packaged
//! language servers run on Bun (see `runtime`), so launch blocks on a
//! non-dismissable dialog until it is installed app-locally.
use super::*;
use gpui_kit::component::{Disableable as _, spinner::Spinner};

/// Live state inside the setup dialog.
struct BunSetupView {
    busy: bool,
    error: Option<String>,
}

impl Render for BunSetupView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .child(
                "Isengard runs its AI agent, code formatter and JavaScript language \
                 servers on Bun. It is downloaded into Isengard's own data folder — \
                 nothing is installed globally.",
            )
            .when(self.busy, |col| {
                col.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Spinner::new().small())
                        .child(div().text_xs().text_color(muted).child("Downloading Bun…")),
                )
            })
            .when_some(self.error.clone(), |col, err| {
                col.child(div().text_xs().text_color(cx.theme().danger).child(err))
            })
    }
}

impl IsengardApp {
    /// Block on the setup dialog when Bun is not installed anywhere we look.
    pub(super) fn check_bun_on_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Tests must never touch the network.
        if cfg!(test) || crate::runtime::bun_path().is_some() {
            return;
        }
        self.show_bun_setup(window, cx);
    }

    pub(super) fn show_bun_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.new(|_| BunSetupView {
            busy: false,
            error: None,
        });
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let (busy, failed) = {
                let v = view.read(cx);
                (v.busy, v.error.is_some())
            };
            let (app_quit, app_install, view_install) = (app.clone(), app.clone(), view.clone());
            center_dialog(
                dialog
                    .title("Set up Bun")
                    .close_button(false)
                    .overlay_closable(false)
                    .keyboard(false)
                    .child(view.clone())
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("bun-quit")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Quit")
                                    .disabled(busy)
                                    .on_click(move |_, _, cx| {
                                        _ = app_quit.update(cx, |this, cx| this.quit(cx));
                                    }),
                            )
                            .child(
                                Button::new("bun-install")
                                    .cursor_pointer()
                                    .primary()
                                    .label(if failed { "Retry" } else { "Install" })
                                    .loading(busy)
                                    .on_click(move |_, window, cx| {
                                        let view = view_install.clone();
                                        _ = app_install.update(cx, |this, cx| {
                                            this.install_bun(view, window, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    fn install_bun(
        &mut self,
        view: Entity<BunSetupView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(recipe) = recipe_for_command("bun") else {
            return;
        };
        view.update(cx, |v, cx| {
            v.busy = true;
            v.error = None;
            cx.notify();
        });
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_spawn(async move { run_install(&recipe) })
                .await;
            let _ = this.update_in(window, |this, window, cx| match result {
                Ok(()) => {
                    window.close_dialog(cx);
                    notify_success("Bun installed", window, cx);
                    this.lsp.invalidate_ready_cache();
                    this.focus_active_editor(window, cx);
                }
                Err(err) => view.update(cx, |v, cx| {
                    v.busy = false;
                    v.error = Some(err);
                    cx.notify();
                }),
            });
        })
        .detach();
    }
}
