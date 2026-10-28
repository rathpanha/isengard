//! Update checks and installs: availability dialog, download progress,
//! restart (self-installed copies), or a sudo-password dialog that installs
//! the RPM/DEB for the user and restarts.
use super::*;
use crate::update::{self, Asset, InstallKind, Progress, Release};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::progress::Progress as ProgressBar;
use std::sync::Mutex;

const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const NOTES_MAX_CHARS: usize = 1200;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Id of the "Checking for updates…" toast, so the result can replace it.
struct CheckingForUpdates;

/// What the download task produced.
enum Staged {
    /// The binary at this path was replaced; restart to run it.
    Installed(PathBuf),
    /// A verified package is waiting at this path.
    Package { path: PathBuf, manager: &'static str },
}

/// Live progress inside the download dialog.
struct DownloadView {
    progress: Arc<Progress>,
    done: bool,
}

impl Render for DownloadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let done = self.progress.done.load(Ordering::Relaxed);
        let total = self.progress.total.load(Ordering::Relaxed);
        let mb = |n: u64| n as f64 / (1024.0 * 1024.0);
        let label = if done == 0 {
            "Checking signature…".to_string()
        } else if total > 0 {
            format!("{:.1} / {:.1} MB", mb(done), mb(total))
        } else {
            format!("{:.1} MB", mb(done))
        };
        v_flex()
            .gap_2()
            .child(ProgressBar::new("update-progress").value(self.progress.percent().unwrap_or(0.)))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
    }
}

/// Live package-manager output inside the install dialog.
struct InstallView {
    lines: Arc<Mutex<Vec<String>>>,
    done: bool,
}

impl Render for InstallView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines.lock().map(|l| l.clone()).unwrap_or_default();
        let shown = lines.len().saturating_sub(6);
        v_flex()
            .gap_2()
            .child(ProgressBar::new("update-install-progress").loading(true))
            .child(
                v_flex()
                    .h_24()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .children(lines.into_iter().skip(shown).map(|l| div().child(l))),
            )
    }
}

impl IsengardApp {
    /// Startup check: at most once a day, silent unless a newer version exists.
    pub(super) fn check_for_updates_on_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Tests must never touch the network.
        if cfg!(test) || !self.config.updates.check {
            return;
        }
        let last = self.config.updates.last_check.unwrap_or(0);
        if now_secs().saturating_sub(last) < CHECK_INTERVAL_SECS {
            return;
        }
        self.run_update_check(false, window, cx);
    }

    pub(super) fn on_check_for_updates(
        &mut self,
        _: &CheckForUpdates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_update_check(true, window, cx);
    }

    fn run_update_check(&mut self, manual: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_busy {
            if manual {
                notify_warning("An update check or download is already running", window, cx);
            }
            return;
        }
        self.update_busy = true;
        if manual {
            // Answer the click right away; removed when the result lands.
            window.push_notification(
                Notification::new()
                    .id::<CheckingForUpdates>()
                    .message("Checking for updates…")
                    .autohide(false),
                cx,
            );
        }
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_spawn(async { update::fetch_latest() })
                .await;
            let _ = this.update_in(window, |this, window, cx| {
                this.update_busy = false;
                if manual {
                    window.remove_notification::<CheckingForUpdates>(cx);
                }
                this.finish_update_check(result, manual, window, cx);
            });
        })
        .detach();
    }

    pub(super) fn finish_update_check(
        &mut self,
        result: anyhow::Result<Release>,
        manual: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let release = match result {
            Ok(release) => release,
            Err(err) => {
                log::warn!("update check failed: {err:#}");
                if manual {
                    notify_error(format!("Update check failed: {err:#}"), window, cx);
                }
                return;
            }
        };
        self.config.updates.last_check = Some(now_secs());
        self.config.save();
        if !update::is_newer(&release.version, update::current_version()) {
            if manual {
                notify_success(
                    format!("Isengard {} is up to date", update::current_version()),
                    window,
                    cx,
                );
            }
            return;
        }
        if !manual && self.config.updates.skipped_version.as_deref() == Some(&release.version) {
            return;
        }
        self.show_update_available(
            release,
            update::detect_install_kind(),
            update::signing_configured(),
            window,
            cx,
        );
    }

    /// The asset to install, or `None` when we can only link to the release.
    fn installable_asset(release: &Release, kind: &InstallKind, signing: bool) -> Option<Asset> {
        if !signing {
            return None;
        }
        update::pick_asset(release, kind).cloned()
    }

    pub(super) fn show_update_available(
        &mut self,
        release: Release,
        kind: InstallKind,
        signing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asset = Self::installable_asset(&release, &kind, signing);
        let primary_label = match (&asset, &kind) {
            (None, _) => "Open release page",
            (Some(_), InstallKind::Package { .. }) => "Download update",
            (Some(_), _) => "Update now",
        };
        let hint = match (&asset, &kind) {
            (Some(_), _) => None,
            (None, InstallKind::Manual) => {
                Some("This copy of Isengard can't update itself. Download the new version from the release page.")
            }
            (None, _) => Some("Automatic install isn't available for this release. Download it from the release page."),
        };
        let release = Rc::new(release);
        let app = cx.entity().downgrade();
        let notes = release_notes_excerpt(&release.notes);
        window.open_dialog(cx, move |dialog, window, cx| {
            let (app_skip, app_primary) = (app.clone(), app.clone());
            let (rel_skip, rel_primary, rel_notes) =
                (release.clone(), release.clone(), release.clone());
            let (asset, kind) = (asset.clone(), kind.clone());
            let mut body = v_flex().gap_2().child(format!(
                "Isengard {} is available. You have {}.",
                release.version,
                update::current_version()
            ));
            if !notes.is_empty() {
                body = body.child(
                    div().max_h_40().overflow_hidden().child(
                        TextView::markdown("update-notes", notes.clone())
                            .style(theme::text_view_style(cx)),
                    ),
                );
            }
            body = body.child(
                Button::new("update-release-notes")
                    .link()
                    .label("View release notes")
                    .on_click(move |_, _, _| {
                        let _ = open::that(&rel_notes.page_url);
                    }),
            );
            if let Some(hint) = hint {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(hint),
                );
            }
            center_dialog(
                dialog.title("Update available").child(body).footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("update-skip")
                                .cursor_pointer()
                                .outline()
                                .label("Skip this version")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let version = rel_skip.version.clone();
                                    _ = app_skip.update(cx, |this, _| {
                                        this.config.updates.skipped_version = Some(version);
                                        this.config.save();
                                    });
                                }),
                        )
                        .child(
                            Button::new("update-later")
                                .cursor_pointer()
                                .outline()
                                .label("Later")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("update-primary")
                                .cursor_pointer()
                                .primary()
                                .label(primary_label)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let Some(asset) = asset.clone() else {
                                        let _ = open::that(update::RELEASES_URL);
                                        return;
                                    };
                                    let (release, kind) = ((*rel_primary).clone(), kind.clone());
                                    _ = app_primary.update(cx, |this, cx| {
                                        this.start_update_download(release, kind, asset, window, cx);
                                    });
                                }),
                        ),
                ),
                window,
            )
        });
    }

    fn start_update_download(
        &mut self,
        release: Release,
        kind: InstallKind,
        asset: Asset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        let progress = Arc::new(Progress::default());
        let view = cx.new(|_| DownloadView {
            progress: progress.clone(),
            done: false,
        });
        let version = release.version.clone();
        let cancel_progress = progress.clone();
        let dialog_view = view.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            let cancel = cancel_progress.clone();
            center_dialog(
                dialog
                    .title(format!("Downloading Isengard {version}"))
                    .close_button(false)
                    .overlay_closable(false)
                    .keyboard(false)
                    .child(dialog_view.clone())
                    .footer(
                        DialogFooter::new().gap_2().child(
                            Button::new("update-cancel")
                                .cursor_pointer()
                                .outline()
                                .label("Cancel")
                                .on_click(move |_, window, cx| {
                                    cancel.cancel.store(true, Ordering::Relaxed);
                                    window.close_dialog(cx);
                                }),
                        ),
                    ),
                window,
            )
        });

        // Repaint the progress bar while the download thread runs.
        let weak_view = view.downgrade();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let done = weak_view
                    .update(cx, |view, cx| {
                        cx.notify();
                        view.done
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();

        let task_progress = progress.clone();
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_spawn(async move {
                    let path = update::download_verified(&release, &asset, &task_progress)?;
                    match kind {
                        InstallKind::InPlace { exe } => {
                            update::install_in_place(&path, &exe)?;
                            let _ = std::fs::remove_file(&path);
                            Ok(Staged::Installed(exe))
                        }
                        InstallKind::WinExe { exe } => {
                            update::install_windows_exe(&path, &exe)?;
                            Ok(Staged::Installed(exe))
                        }
                        InstallKind::MacApp { bundle } => {
                            let exe = update::install_app_bundle(&path, &bundle)?;
                            Ok(Staged::Installed(exe))
                        }
                        InstallKind::Package { manager, .. } => {
                            Ok(Staged::Package { path, manager })
                        }
                        InstallKind::Manual => anyhow::bail!("this install cannot be updated"),
                    }
                })
                .await;
            let _ = this.update_in(window, |this, window, cx| {
                this.update_busy = false;
                view.update(cx, |view, _| view.done = true);
                // Cancel already closed the dialog and means "say nothing".
                if progress.cancel.load(Ordering::Relaxed) {
                    return;
                }
                window.close_dialog(cx);
                match result {
                    Ok(Staged::Installed(exe)) => this.show_update_restart(exe, window, cx),
                    Ok(Staged::Package { path, manager }) => {
                        this.show_update_install_command(path, manager, window, cx);
                    }
                    Err(err) => {
                        log::warn!("update failed: {err:#}");
                        notify_error(
                            format!(
                                "Update failed: {err:#}. You can download it from the release page."
                            ),
                            window,
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn show_update_restart(&mut self, exe: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            let app = app.clone();
            let exe = exe.clone();
            center_dialog(
                dialog
                    .title("Update ready")
                    .child(
                        "The new version is installed and verified. Restart to finish \
                         updating — your open workspace and tabs will be restored.",
                    )
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("update-later")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Later")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                Button::new("update-restart")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Restart now")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let exe = exe.clone();
                                        _ = app.update(cx, |this, cx| {
                                            this.restart_into(exe, window, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    /// Quit (through the usual unsaved-changes prompts) and start `exe` again.
    pub(super) fn restart_into(&mut self, exe: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.relaunch_exe = Some(exe);
        if self.confirm_quit(window, cx) {
            self.quit(cx);
        }
    }

    /// Start the replaced binary on the open workspace. Called from `quit`.
    pub(super) fn relaunch(&self, exe: &Path) {
        let mut command = std::process::Command::new(exe);
        if let Some(target) = self
            .workspace
            .file()
            .map(Path::to_path_buf)
            .or_else(|| self.workspace.folders().first().cloned())
        {
            command.arg(target);
        }
        if let Err(err) = command.spawn() {
            log::error!("relaunch {} failed: {err}", exe.display());
        }
    }

    pub(super) fn show_update_install_command(
        &mut self,
        package: PathBuf,
        manager: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = update::install_command(manager, &package);
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let (app_term, command_term, command_copy) =
                (app.clone(), command.clone(), command.clone());
            let (app_install, package_install) = (app.clone(), package.clone());
            let muted = cx.theme().muted_foreground;
            let body = v_flex()
                .gap_2()
                .child("The update was downloaded and its signature and checksum verified.")
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(package.display().to_string()),
                )
                .child("Installing needs administrator rights. Isengard can install it for you (it asks for your password and restarts afterwards), or you can run this command yourself:")
                .child(
                    div()
                        .p_2()
                        .bg(cx.theme().muted)
                        .text_sm()
                        .child(command.clone()),
                );
            center_dialog(
                dialog.title("Update downloaded").child(body).footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("update-later")
                                .cursor_pointer()
                                .outline()
                                .label("Later")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("update-copy")
                                .cursor_pointer()
                                .outline()
                                .label("Copy command")
                                .on_click(move |_, window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        command_copy.clone(),
                                    ));
                                    window.close_dialog(cx);
                                    notify_success("Install command copied", window, cx);
                                }),
                        )
                        .child(
                            Button::new("update-terminal")
                                .cursor_pointer()
                                .outline()
                                .label("Open in terminal")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let command = command_term.clone();
                                    _ = app_term.update(cx, |this, cx| {
                                        this.type_command_in_terminal(&command, window, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("update-install")
                                .cursor_pointer()
                                .primary()
                                .label("Install now")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let package = package_install.clone();
                                    _ = app_install.update(cx, |this, cx| {
                                        this.show_update_password(package, manager, None, window, cx);
                                    });
                                }),
                        ),
                ),
                window,
            )
        });
    }

    /// Ask for the sudo password. `error` is shown after a rejected attempt.
    pub(super) fn show_update_password(
        &mut self,
        package: PathBuf,
        manager: &'static str,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Password"));
        input.update(cx, |state, cx| {
            state.set_masked(true, window, cx);
            state.focus(window, cx);
        });
        let (app_enter, package_enter, input_enter) =
            (cx.entity().downgrade(), package.clone(), input.clone());
        cx.subscribe_in(&input, window, move |_, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                // This runs inside the input's own update: reading it or
                // dropping its dialog here is a re-entrant lease. Wait for
                // the emit to finish.
                let (app, package, input) = (app_enter.clone(), package_enter.clone(), input_enter.clone());
                window.defer(cx, move |window, cx| {
                    let password = input.read(cx).value().to_string();
                    window.close_dialog(cx);
                    _ = app.update(cx, |this, cx| {
                        this.start_package_install(package, manager, password, window, cx);
                    });
                });
            }
        })
        .detach();
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let (app_ok, package_ok, input_ok) = (app.clone(), package.clone(), input.clone());
            let mut body = v_flex()
                .gap_2()
                .child("Installing the update needs administrator rights. Enter your password to continue.")
                .child(
                    Input::new(&input)
                        .id("update-password")
                        .mask_toggle()
                        .w_full(),
                );
            if let Some(error) = &error {
                body = body.child(div().text_xs().text_color(cx.theme().danger).child(error.clone()));
            }
            center_dialog(
                dialog.title("Administrator password").child(body).footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("update-password-cancel")
                                .cursor_pointer()
                                .outline()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("update-password-ok")
                                .cursor_pointer()
                                .primary()
                                .label("Install")
                                .on_click(move |_, window, cx| {
                                    let password = input_ok.read(cx).value().to_string();
                                    window.close_dialog(cx);
                                    let package = package_ok.clone();
                                    _ = app_ok.update(cx, |this, cx| {
                                        this.start_package_install(package, manager, password, window, cx);
                                    });
                                }),
                        ),
                ),
                window,
            )
        });
    }

    /// Run the package install in the background, show its output, then
    /// restart into the new binary.
    fn start_package_install(
        &mut self,
        package: PathBuf,
        manager: &'static str,
        password: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        // After the package replaces the file, current_exe() reads "… (deleted)".
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.trim_end_matches(" (deleted)").to_string()))
            .map_or_else(|| PathBuf::from("/usr/bin/isengard"), PathBuf::from);
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let view = cx.new(|_| InstallView {
            lines: lines.clone(),
            done: false,
        });
        let dialog_view = view.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            center_dialog(
                dialog
                    .title("Installing update")
                    .close_button(false)
                    .overlay_closable(false)
                    .keyboard(false)
                    .child(dialog_view.clone()),
                window,
            )
        });
        let weak_view = view.downgrade();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let done = weak_view
                    .update(cx, |view, cx| {
                        cx.notify();
                        view.done
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();

        let task_package = package.clone();
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_spawn(async move {
                    update::install_package(manager, &task_package, &password, |line| {
                        if let Ok(mut lines) = lines.lock() {
                            lines.push(line.to_string());
                        }
                    })
                })
                .await;
            let _ = this.update_in(window, |this, window, cx| {
                this.update_busy = false;
                view.update(cx, |view, _| view.done = true);
                window.close_dialog(cx);
                match result {
                    Ok(()) => {
                        // Installed; the downloaded package is no longer needed.
                        let _ = std::fs::remove_file(&package);
                        notify_success("Update installed — restarting Isengard", window, cx);
                        this.restart_into(exe, window, cx);
                    }
                    Err(update::InstallError::WrongPassword) => {
                        let error = Some("Incorrect password. Try again.".to_string());
                        this.show_update_password(package, manager, error, window, cx);
                    }
                    Err(update::InstallError::Failed(output)) => {
                        log::warn!("package install failed: {output}");
                        notify_error(format!("Install failed: {output}"), window, cx);
                        this.show_update_install_command(package, manager, window, cx);
                    }
                }
            });
        })
        .detach();
    }

    /// New terminal tab with `command` typed at the prompt — not executed,
    /// so the user presses Enter and enters their own sudo password.
    pub(super) fn type_command_in_terminal(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folders = self.workspace.folders().to_vec();
        if folders.is_empty() {
            return notify_error(
                "Open a folder to use the built-in terminal, or use Copy command and run it anywhere",
                window,
                cx,
            );
        }
        let cwd = crate::terminal::pty::default_cwd(&folders);
        if let Err(err) = self
            .terminal
            .update(cx, |term, cx| term.add_session(cwd, window, cx))
        {
            return notify_error(err, window, cx);
        }
        self.terminal
            .update(cx, |term, cx| term.type_in_active(command, cx));
        self.persist_session(cx);
        cx.notify();
    }
}

/// First lines of the release notes, bounded so the dialog stays small.
fn release_notes_excerpt(notes: &str) -> String {
    let trimmed = notes.trim();
    if trimmed.chars().count() <= NOTES_MAX_CHARS {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(NOTES_MAX_CHARS).collect();
    format!("{}…", cut.trim_end())
}
