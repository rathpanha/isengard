//! JDK selection for Java projects. Isengard never installs a JDK: it scans
//! for the ones the user has (`crate::jdk`), offers a per-workspace picker in
//! the status bar, and shows a dialog when none exists.
use super::*;

impl IsengardApp {
    fn ensure_jdks(&mut self) {
        // Tests must not depend on the machine's JDKs.
        if self.jdks_scanned || cfg!(test) {
            return;
        }
        self.jdks_scanned = true;
        self.jdks = crate::jdk::scan(&self.config.java.homes);
    }

    /// The JDK in use for this workspace.
    pub(super) fn active_jdk(&self) -> Option<&crate::jdk::Jdk> {
        crate::jdk::choose(&self.jdks, self.java_home.as_deref())
    }

    /// Show the JDK item: a Java file is active, or the folders hold a build file.
    pub(super) fn java_context(&self) -> bool {
        self.java_project
            || self
                .tabs
                .active()
                .is_some_and(|t| t.language() == Some(Language::Java))
    }

    /// First Java context with no JDK installed → tell the user (once per run).
    pub(super) fn check_jdk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.java_context() {
            return;
        }
        self.ensure_jdks();
        if self.jdks.is_empty() && !self.jdk_missing_shown && !cfg!(test) {
            self.show_jdk_missing(window, cx);
        }
    }

    pub(super) fn show_jdk_missing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.jdk_missing_shown = true;
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            let (app_scan, app_add) = (app.clone(), app.clone());
            center_dialog(
                dialog
                    .title("No JDK found")
                    .child(
                        "Java projects need a JDK (21 or newer recommended). Isengard does \
                         not install one for you — install a JDK, then Re-scan, or point \
                         Isengard at one you already have.",
                    )
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("jdk-download")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Download JDK")
                                    .on_click(|_, _, _| {
                                        let _ = open::that_detached(crate::jdk::DOWNLOAD_URL);
                                    }),
                            )
                            .child(
                                Button::new("jdk-add-existing")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Add JDK…")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        _ = app_add.update(cx, |this, cx| this.add_jdk(window, cx));
                                    }),
                            )
                            .child(
                                Button::new("jdk-rescan-dialog")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Re-scan")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        _ = app_scan
                                            .update(cx, |this, cx| this.rescan_jdks(window, cx));
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    /// Re-run discovery (after the user installed a JDK). Still none → ask again.
    pub(super) fn rescan_jdks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.jdks_scanned = false;
        self.ensure_jdks();
        if self.jdks.is_empty() {
            self.show_jdk_missing(window, cx);
        } else {
            notify_success(format!("Found {} JDK(s)", self.jdks.len()), window, cx);
            self.jdtls_old_jdk_warned = false;
            self.refresh_java_launch(window, cx);
        }
        cx.notify();
    }

    /// Feed the JDKs to the language host. jdtls needs a JDK 21+ to run; when
    /// only older ones exist, say so once instead of failing silently.
    pub(super) fn refresh_java_launch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_jdks();
        let launch = crate::lsp::JavaLaunch::from_jdks(
            &self.jdks,
            self.active_jdk(),
            self.config.java.jdtls_xmx.as_deref(),
        );
        if launch.is_none() && !self.jdks.is_empty() && !self.jdtls_old_jdk_warned {
            self.jdtls_old_jdk_warned = true;
            notify_warning(
                format!(
                    "Java IntelliSense needs a JDK {}+ to run its language server (projects \
                     can still target older Java). Install one, then Re-scan.",
                    crate::lsp::JDTLS_MIN_JAVA
                ),
                window,
                cx,
            );
        }
        self.lsp.set_java(launch);
        self.sync_terminal_java_env(cx);
    }

    /// Pick the project JDK; a running jdtls restarts to compile against it.
    pub(super) fn select_jdk(
        &mut self,
        home: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.java_home.as_ref() == Some(&home) {
            return;
        }
        self.java_home = Some(home);
        self.persist_session(cx);
        self.refresh_java_launch(window, cx);
        if self
            .tabs
            .iter()
            .any(|t| t.language() == Some(Language::Java))
        {
            self.restart_lsp_servers(window, cx);
        }
        cx.notify();
    }

    /// Folder picker for a JDK outside the places we scan.
    pub(super) fn add_jdk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add JDK".into()),
        });
        cx.spawn_in(window, async move |this, window| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(chosen) = paths.into_iter().next() else {
                return;
            };
            this.update_in(window, |this, window, cx| {
                this.register_jdk(&chosen, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Validate `home`, remember it globally and make it this workspace's JDK.
    pub(super) fn register_jdk(
        &mut self,
        home: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(jdk) = crate::jdk::inspect(home) else {
            notify_error(
                format!(
                    "Not a JDK (needs bin/javac and a release file): {}",
                    home.display()
                ),
                window,
                cx,
            );
            return;
        };
        if !self.config.java.homes.contains(&jdk.home) {
            self.config.java.homes.push(jdk.home.clone());
            self.config.save();
        }
        let real = jdk.home.canonicalize().unwrap_or_else(|_| jdk.home.clone());
        self.jdks_scanned = false;
        self.ensure_jdks();
        if cfg!(test) && !self.jdks.iter().any(|j| j.home == real) {
            self.jdks.push(crate::jdk::Jdk {
                home: real.clone(),
                ..jdk
            });
        }
        self.select_jdk(real, window, cx);
    }

    /// Run build `build`'s task `task` (menu indices) in a terminal tab.
    pub(super) fn run_java_task(
        &mut self,
        build: usize,
        task: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((build, task)) = self
            .java_builds
            .get(build)
            .and_then(|b| Some((b.clone(), b.tasks.get(task)?.clone())))
        else {
            return;
        };
        self.launch_task(build, task, None, window, cx);
    }

    /// Run a saved run configuration.
    pub(super) fn run_java_config(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = self.java_runs.get(index).cloned() else {
            return;
        };
        let Some(build) = self
            .java_builds
            .iter()
            .find(|b| b.root == config.root)
            .cloned()
        else {
            return notify_error(
                format!(
                    "{} no longer has a Maven / Gradle build",
                    config.root.display()
                ),
                window,
                cx,
            );
        };
        let task = crate::java_tasks::Task {
            label: config.name,
            args: vec![config.args],
        };
        self.launch_task(build, task, None, window, cx);
    }

    /// Re-scan the workspace for runnable `main` classes (off the UI thread).
    pub(super) fn refresh_java_services(&mut self, cx: &mut Context<Self>) {
        self.java_services_gen += 1;
        let generation = self.java_services_gen;
        let folders = self.workspace.folders().to_vec();
        if folders.is_empty() {
            self.java_services.clear();
            return;
        }
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { crate::java_tasks::scan_services(&folders) })
                .await;
            this.update(cx, |this, cx| {
                if this.java_services_gen == generation && this.java_services != found {
                    this.java_services = found;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Start one service from the Run menu.
    pub(super) fn run_java_service(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(service) = self.java_services.get(index).cloned() else {
            return;
        };
        let title = service.name().to_string();
        if let Some(build) = service
            .module
            .as_deref()
            .and_then(crate::java_tasks::detect)
        {
            let task = service.task(&build);
            return self.launch_task(build, task, Some(title), window, cx);
        }
        self.ensure_jdks();
        let Some(jdk) = self.active_jdk().map(|j| j.home.clone()) else {
            return self.show_jdk_missing(window, cx);
        };
        let command = crate::java_tasks::single_file_command(&service.main, Some(&jdk));
        let cwd = service
            .main
            .file
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        self.spawn_task_tab(cwd, title, command, window, cx);
    }

    /// Ctrl+C one running task tab.
    pub(super) fn stop_java_task(&mut self, id: usize, cx: &mut Context<Self>) {
        self.terminal
            .update(cx, |term, cx| term.interrupt_task(id, cx));
    }

    /// Show a task's terminal tab.
    pub(super) fn focus_java_task(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal.update(cx, |term, cx| {
            term.select_tab(id, window, cx);
            term.show(window, cx);
        });
        cx.notify();
    }

    /// Ctrl+C every running task tab.
    pub(super) fn stop_java_tasks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stopped = self
            .terminal
            .update(cx, |term, cx| term.interrupt_tasks(cx));
        if stopped == 0 {
            notify_warning("Nothing is running", window, cx);
        }
    }

    fn launch_task(
        &mut self,
        build: crate::java_tasks::Build,
        task: crate::java_tasks::Task,
        title: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_jdks();
        let Some(jdk) = self.active_jdk().map(|j| j.home.clone()) else {
            return self.show_jdk_missing(window, cx);
        };
        let command = build.command(&task, Some(&jdk));
        let title = title.unwrap_or_else(|| build.title(&task));
        // A wrapper always exists; otherwise make sure the tool is installed
        // before opening a tab that would only say "command not found".
        if build.wrapper || cfg!(test) {
            return self.spawn_task_tab(build.root, title, command, window, cx);
        }
        let tool = build.tool_name();
        cx.spawn_in(window, async move |this, window| {
            let found = window
                .background_spawn(async move { crate::java_tasks::tool_on_path(tool) })
                .await;
            this.update_in(window, |this, window, cx| {
                if found {
                    this.spawn_task_tab(build.root, title, command, window, cx);
                } else {
                    notify_error(
                        format!(
                            "`{tool}` was not found. Install it, or add the project wrapper \
                             (`{}`).",
                            if tool == "mvn" {
                                "mvn wrapper:wrapper"
                            } else {
                                "gradle wrapper"
                            }
                        ),
                        window,
                        cx,
                    );
                }
            })
            .ok();
        })
        .detach();
    }

    fn spawn_task_tab(
        &mut self,
        cwd: PathBuf,
        title: String,
        command: crate::terminal::pty::TerminalCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.terminal.update(cx, |term, cx| {
            term.add_task_session(cwd, title, &command, window, cx)
        });
        if let Err(err) = result {
            return notify_error(err, window, cx);
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// Plain terminals opened from now on start with the project JDK in
    /// `JAVA_HOME` / `PATH` (shell rc files may still override it; task tabs
    /// export it in the command line and always win).
    pub(super) fn sync_terminal_java_env(&mut self, cx: &mut Context<Self>) {
        let env = match (self.java_project, self.active_jdk()) {
            (true, Some(jdk)) => {
                let home = jdk.home.to_string_lossy().into_owned();
                let path = std::env::var_os("PATH").unwrap_or_default();
                let joined = std::env::join_paths(
                    std::iter::once(jdk.home.join("bin")).chain(std::env::split_paths(&path)),
                )
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
                vec![
                    ("JAVA_HOME".to_string(), home),
                    ("PATH".to_string(), joined),
                ]
            }
            _ => Vec::new(),
        };
        self.terminal.update(cx, |term, _| term.set_shell_env(env));
    }

    /// Dialog: name + Maven/Gradle arguments for a new run configuration.
    pub(super) fn add_run_config(
        &mut self,
        build: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(build) = self.java_builds.get(build).cloned() else {
            return;
        };
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name, e.g. dev"));
        let args = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match build.tool {
                crate::java_tasks::Tool::Maven => {
                    "Goals, e.g. spring-boot:run -Dspring-boot.run.profiles=dev"
                }
                crate::java_tasks::Tool::Gradle => {
                    "Tasks, e.g. bootRun --args='--spring.profiles.active=dev'"
                }
            })
        });
        name.update(cx, |input, cx| input.focus(window, cx));
        let app = cx.entity().downgrade();
        let root = build.root.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            let app_ok = app.clone();
            let (name_ok, args_ok) = (name.clone(), args.clone());
            let root = root.clone();
            center_dialog(
                dialog
                    .title("New Run Configuration")
                    .child(
                        v_flex()
                            .gap_2()
                            .child(Input::new(&name).w_full().min_w(px(360.)))
                            .child(Input::new(&args).w_full()),
                    )
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("java-config-cancel")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                Button::new("java-config-save")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Save")
                                    .on_click(move |_, window, cx| {
                                        let name = name_ok.read(cx).value().trim().to_string();
                                        let args = args_ok.read(cx).value().trim().to_string();
                                        if name.is_empty() || args.is_empty() {
                                            return notify_warning(
                                                "Give the configuration a name and arguments",
                                                window,
                                                cx,
                                            );
                                        }
                                        window.close_dialog(cx);
                                        let root = root.clone();
                                        _ = app_ok.update(cx, |this, cx| {
                                            this.save_run_config(name, root, args, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    pub(super) fn save_run_config(
        &mut self,
        name: String,
        root: PathBuf,
        args: String,
        cx: &mut Context<Self>,
    ) {
        self.java_runs
            .retain(|r| !(r.root == root && r.name == name));
        self.java_runs
            .push(crate::config::RunConfig { name, root, args });
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn remove_run_config(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.java_runs.len() {
            self.java_runs.remove(index);
            self.persist_session(cx);
            cx.notify();
        }
    }
}
