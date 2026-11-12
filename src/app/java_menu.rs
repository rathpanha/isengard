//! Status-bar menus for Java: one per build tool (Maven, Gradle — tasks and
//! saved run configurations) and one for Services (running + runnable classes).
use super::*;
use crate::file_tree::material_icon;
use crate::java_tasks::{Build, Service, Tool};
use crate::terminal::panel::{RunningTask, TaskPhase};
use gpui_kit::component::Icon;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::spinner::Spinner;
use std::collections::HashSet;

/// Which Java status-bar popover is open (at most one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JavaMenu {
    Build(Tool),
    Services,
}

type AppAction = Box<dyn Fn(&mut IsengardApp, &mut Window, &mut Context<IsengardApp>)>;
type ClickAction = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Click handlers that reach the app from inside a popover.
struct MenuActions {
    app: WeakEntity<IsengardApp>,
    popover: WeakEntity<PopoverState>,
}

impl MenuActions {
    /// Close the popover, then run `f` on the app.
    fn act(&self, f: AppAction) -> ClickAction {
        let (app, popover) = (self.app.clone(), self.popover.clone());
        Box::new(move |_, window, cx| {
            if let Some(p) = popover.upgrade() {
                p.update(cx, |p, cx| p.dismiss(window, cx));
            }
            _ = app.update(cx, |this, cx| {
                this.java_menu = None;
                f(this, window, cx);
            });
        })
    }

    /// Same, but the popover stays open (stopping a service, deleting a config).
    fn keep_open(&self, f: AppAction) -> ClickAction {
        let app = self.app.clone();
        Box::new(move |_, window, cx| {
            _ = app.update(cx, |this, cx| f(this, window, cx));
        })
    }
}

/// Left-aligned, full-width menu row. Kit's label-only `Button` centres its
/// text, so menus built from it would not line up with icon rows.
fn menu_row(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let hover = cx.theme().accent.opacity(0.15);
    h_flex()
        .id(id)
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .text_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

/// Like [`menu_row`] but not a click target (no hover fill, no pointer).
fn plain_row(id: impl Into<ElementId>) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .text_sm()
}

fn menu_heading(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// Rows (headings included) beyond which a menu stops growing and scrolls.
const MENU_MAX_ROWS: usize = 14;
const MENU_SCROLL_HEIGHT: f32 = 440.;
const MENU_WIDTH: f32 = 400.;

/// Popover body. A popover is auto-height, so `max_h` alone never makes the
/// list overflow (the scroll wrapper just grows): once the list is long it
/// gets a definite height and scrolls with a scrollbar.
fn menu_body(rows: usize) -> Div {
    v_flex()
        .w(px(MENU_WIDTH))
        .gap_1()
        .when(rows > MENU_MAX_ROWS, |col| col.h(px(MENU_SCROLL_HEIGHT)))
}

impl IsengardApp {
    fn java_popover(&self, menu: JavaMenu, id: &'static str, cx: &mut Context<Self>) -> Popover {
        Popover::new(id)
            .anchor(Anchor::BottomRight)
            .open(self.java_menu == Some(menu))
            .on_open_change(cx.listener(move |this, open, window, cx| {
                if *open {
                    this.java_menu = Some(menu);
                } else if this.java_menu == Some(menu) {
                    this.java_menu = None;
                    this.focus_active_editor(window, cx);
                }
                cx.notify();
            }))
    }

    /// Maven or Gradle: tasks per build folder plus saved run configurations.
    pub(super) fn java_build_menu(&self, tool: Tool, cx: &mut Context<Self>) -> AnyElement {
        let builds: Vec<(usize, Build)> = self
            .java_builds
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, b)| b.tool == tool)
            .collect();
        let runs = self.java_runs.clone();
        let app = cx.entity().downgrade();
        let (id, trigger_id, icon, name) = match tool {
            Tool::Maven => ("java-maven", "status-maven", "maven", "Maven"),
            Tool::Gradle => ("java-gradle", "status-gradle", "gradle", "Gradle"),
        };
        self.java_popover(JavaMenu::Build(tool), id, cx)
            .trigger(
                Button::new(trigger_id)
                    .ghost()
                    .cursor_pointer()
                    .small()
                    .tooltip(name)
                    .child(material_icon(icon, 16.)),
            )
            .content(move |_state: &mut PopoverState, _, cx| {
                let actions = MenuActions {
                    app: app.clone(),
                    popover: cx.weak_entity(),
                };
                build_menu_body(&actions, &builds, &runs, cx)
            })
            .into_any_element()
    }

    /// Everything running (port + stop) and every class that can be started.
    pub(super) fn java_services_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let running = self.terminal.read(cx).running_tasks();
        let services = self.java_services.clone();
        let multi = self.java_multi_services.clone();
        let expanded = self.java_expanded.clone();
        let launching: HashSet<String> = self.java_launching.keys().cloned().collect();
        let app = cx.entity().downgrade();
        let busy = !running.is_empty();
        let visible = busy || !services.is_empty();
        div()
            .when(visible, |this| {
                this.child(
                    self.java_popover(JavaMenu::Services, "java-services", cx)
                        .trigger(
                            Button::new("status-java-services")
                                .ghost()
                                .cursor_pointer()
                                .small()
                                .icon(if busy {
                                    LucideIcon::Loader
                                } else {
                                    LucideIcon::Play
                                })
                                .tooltip("Services"),
                        )
                        .content(move |_state: &mut PopoverState, _, cx| {
                            let actions = MenuActions {
                                app: app.clone(),
                                popover: cx.weak_entity(),
                            };
                            services_menu_body(
                                &actions, &running, &services, &multi, &expanded, &launching, cx,
                            )
                        }),
                )
            })
            .into_any_element()
    }
}

/// Running instances grouped by what started them: one entry per service /
/// task, in start order.
fn group_running(running: &[RunningTask]) -> Vec<(String, Vec<&RunningTask>)> {
    let mut groups: Vec<(String, Vec<&RunningTask>)> = Vec::new();
    for task in running {
        match groups.iter_mut().find(|(key, _)| *key == task.group) {
            Some((_, list)) => list.push(task),
            None => groups.push((task.group.clone(), vec![task])),
        }
    }
    groups
}

/// Muted summary on a group header: the port of a lone instance, else a count.
fn group_summary(instances: &[&RunningTask]) -> String {
    match instances {
        [only] => only
            .port
            .map_or_else(|| "1 running".to_string(), |p| format!(":{p}")),
        many => format!("{} running", many.len()),
    }
}

/// Loading state of a group, from its instances (and launches still waiting
/// for their tab): the control that started the work shows the spinner.
struct GroupBusy {
    /// Play: a launch is pending or an instance is still starting.
    starting: bool,
    /// Stop: an instance is stopping (and is not being restarted).
    stopping: bool,
    /// Restart: an instance is stopping to be started again.
    restarting: bool,
}

impl GroupBusy {
    fn of(instances: &[&RunningTask], launching: bool) -> Self {
        let any = |f: &dyn Fn(&RunningTask) -> bool| instances.iter().any(|t| f(t));
        Self {
            starting: launching || any(&|t| t.phase == TaskPhase::Starting),
            stopping: any(&|t| t.phase == TaskPhase::Stopping && !t.restarting),
            restarting: any(&|t| t.restarting),
        }
    }
}

/// Status mark of an instance: spinner while starting / stopping, else a green square.
fn phase_mark(phase: TaskPhase, cx: &App) -> AnyElement {
    match phase {
        TaskPhase::Running => div()
            .size(px(7.))
            .flex_shrink_0()
            .bg(cx.theme().success)
            .into_any_element(),
        TaskPhase::Starting | TaskPhase::Stopping => Spinner::new().small().into_any_element(),
    }
}

/// Header of a group (service or task): play, name, summary, "multiple
/// instances" checkbox, restart, stop-all, and the accordion chevron at the
/// far right. Only the play button starts something — the row itself is not a
/// click target. `start` / `multi` are `Some` for services only (restart too:
/// only services start again). Optional items simply come and go: nothing is
/// padded, and the play button is always the first item.
#[allow(clippy::too_many_arguments)]
fn group_header(
    actions: &MenuActions,
    id: SharedString,
    key: &str,
    name: String,
    instances: &[&RunningTask],
    launching: bool,
    open: bool,
    start: Option<AppAction>,
    multi: Option<bool>,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    let toggle_key = key.to_string();
    let stop_key = key.to_string();
    let restart_key = key.to_string();
    let multi_key = key.to_string();
    let multi_app = actions.app.clone();
    let on_toggle = actions.keep_open(Box::new(move |this, _, cx| {
        this.toggle_java_group(&toggle_key, cx)
    }));
    let on_stop = actions.keep_open(Box::new(move |this, _, cx| {
        this.stop_java_group(&stop_key, cx)
    }));
    let on_restart = actions.keep_open(Box::new(move |this, _, cx| {
        this.restart_java_group(&restart_key, cx)
    }));
    // The menu stays open so several services can be started in a row.
    let can_restart = start.is_some();
    let on_start = start.map(|f| actions.keep_open(f));
    let running = !instances.is_empty();
    let summary = group_summary(instances);
    let busy = GroupBusy::of(instances, launching);
    plain_row(SharedString::from(format!("{id}-row")))
        .when_some(on_start, |row, on_start| {
            row.child(
                Button::new(id.clone())
                    .ghost()
                    .cursor_pointer()
                    .xsmall()
                    .icon(LucideIcon::Play)
                    .loading(busy.starting)
                    .tooltip("Start")
                    .on_click(move |e, window, cx| on_start(e, window, cx)),
            )
        })
        .child(div().flex_1().min_w_0().truncate().child(name))
        .when(running, |row| {
            row.child(div().text_color(muted).flex_shrink_0().child(summary))
        })
        .when_some(multi, |row, on| {
            row.child(
                Checkbox::new(SharedString::from(format!("{id}-multi")))
                    .small()
                    .checked(on)
                    .tooltip("Allow multiple instances")
                    .cursor_pointer()
                    .on_click(move |checked: &bool, _, cx| {
                        let on = *checked;
                        _ = multi_app
                            .update(cx, |this, cx| this.set_java_multi(&multi_key, on, cx));
                    }),
            )
        })
        .when(running && can_restart, |row| {
            row.child(
                Button::new(SharedString::from(format!("{id}-restart")))
                    .ghost()
                    .cursor_pointer()
                    .xsmall()
                    .icon(LucideIcon::RotateCw)
                    .loading(busy.restarting)
                    .tooltip("Restart")
                    .on_click(move |e, window, cx| on_restart(e, window, cx)),
            )
        })
        .when(running, |row| {
            row.child(
                Button::new(SharedString::from(format!("{id}-stop")))
                    .ghost()
                    .cursor_pointer()
                    .xsmall()
                    .icon(LucideIcon::CircleX)
                    .loading(busy.stopping)
                    .tooltip("Stop all instances")
                    .on_click(move |e, window, cx| on_stop(e, window, cx)),
            )
            .child(
                Button::new(SharedString::from(format!("{id}-toggle")))
                    .ghost()
                    .cursor_pointer()
                    .xsmall()
                    .icon(if open {
                        LucideIcon::ChevronDown
                    } else {
                        LucideIcon::ChevronRight
                    })
                    .tooltip(if open {
                        "Hide instances"
                    } else {
                        "Show instances"
                    })
                    .on_click(move |e, window, cx| on_toggle(e, window, cx)),
            )
        })
        .test_support()
}

/// One running instance under its group header.
fn instance_row(
    actions: &MenuActions,
    task: &RunningTask,
    can_restart: bool,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    let id = task.id;
    // The menu stays open: it is a control panel for what runs.
    let on_focus = actions.keep_open(Box::new(move |this, window, cx| {
        this.focus_java_task(id, window, cx)
    }));
    let on_stop = actions.keep_open(Box::new(move |this, _, cx| this.stop_java_task(id, cx)));
    let on_restart = actions.keep_open(Box::new(move |this, _, cx| this.restart_java_task(id, cx)));
    menu_row(SharedString::from(format!("java-running-{id}")), cx)
        .pl(px(40.))
        .on_click(move |e, window, cx| on_focus(e, window, cx))
        .child(phase_mark(task.phase, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(task.title.clone()),
        )
        .when_some(task.port, |row, port| {
            row.child(div().text_color(muted).child(format!(":{port}")))
        })
        .when(can_restart, |row| {
            row.child(
                Button::new(SharedString::from(format!("java-running-restart-{id}")))
                    .ghost()
                    .cursor_pointer()
                    .xsmall()
                    .icon(LucideIcon::RotateCw)
                    .loading(task.restarting)
                    .tooltip("Restart")
                    .on_click(move |e, window, cx| {
                        cx.stop_propagation();
                        on_restart(e, window, cx)
                    }),
            )
        })
        .child(
            Button::new(SharedString::from(format!("java-running-stop-{id}")))
                .ghost()
                .cursor_pointer()
                .xsmall()
                .icon(LucideIcon::CircleX)
                .loading(task.phase == TaskPhase::Stopping && !task.restarting)
                .tooltip("Stop")
                .on_click(move |e, window, cx| {
                    cx.stop_propagation();
                    on_stop(e, window, cx)
                }),
        )
        .test_support()
}

fn services_menu_body(
    actions: &MenuActions,
    running: &[RunningTask],
    services: &[Service],
    multi: &HashSet<String>,
    expanded: &HashSet<String>,
    launching: &HashSet<String>,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let groups = group_running(running);
    let service_keys: Vec<String> = services.iter().map(Service::group_key).collect();
    // Tasks that are not services (`mvn clean`, run configurations).
    let task_groups: Vec<&(String, Vec<&RunningTask>)> = groups
        .iter()
        .filter(|(key, _)| !service_keys.contains(key))
        .collect();
    let open_instances: usize = groups
        .iter()
        .filter(|(key, _)| expanded.contains(key))
        .map(|(_, list)| list.len())
        .sum();
    let rows = 2 + services.len() + task_groups.len() + open_instances;
    menu_body(rows)
        .overflow_y_scrollbar()
        .when(!services.is_empty(), |col| {
            col.child(
                h_flex()
                    .gap_1()
                    .px_1()
                    .child(
                        Button::new("java-start-all")
                            .ghost()
                            .cursor_pointer()
                            .small()
                            .icon(LucideIcon::Play)
                            .label("Start all")
                            .on_click({
                                let on = actions.keep_open(Box::new(|this, window, cx| {
                                    this.start_all_java_services(window, cx)
                                }));
                                move |e, window, cx| on(e, window, cx)
                            }),
                    )
                    .when(!running.is_empty(), |row| {
                        row.child(
                            Button::new("java-stop-all")
                                .ghost()
                                .cursor_pointer()
                                .small()
                                .icon(LucideIcon::CircleX)
                                .label("Stop all")
                                .on_click({
                                    let on = actions.keep_open(Box::new(|this, window, cx| {
                                        this.stop_java_tasks(window, cx)
                                    }));
                                    move |e, window, cx| on(e, window, cx)
                                }),
                        )
                    }),
            )
            .child(menu_heading(
                "Services · checkbox = allow multiple instances",
                cx,
            ))
            .children(services.iter().enumerate().map(|(i, svc)| {
                let key = svc.group_key();
                let instances: Vec<&RunningTask> = groups
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, list)| list.clone())
                    .unwrap_or_default();
                let open = expanded.contains(&key) && !instances.is_empty();
                let header = group_header(
                    actions,
                    SharedString::from(format!("java-service-{i}")),
                    &key,
                    svc.name().to_string(),
                    &instances,
                    launching.contains(&key),
                    open,
                    Some(Box::new(move |this, window, cx| {
                        this.run_java_service(i, window, cx)
                    })),
                    Some(multi.contains(&key)),
                    cx,
                );
                v_flex().child(header).when(open, |col| {
                    col.children(
                        instances
                            .iter()
                            .map(|task| instance_row(actions, task, true, cx)),
                    )
                })
            }))
        })
        .when(!task_groups.is_empty(), |col| {
            col.child(menu_heading("Tasks", cx))
                .children(task_groups.iter().enumerate().map(|(j, (key, list))| {
                    let open = expanded.contains(key);
                    let name = list[0].name.clone();
                    let header = group_header(
                        actions,
                        SharedString::from(format!("java-task-group-{j}")),
                        key,
                        name,
                        list,
                        launching.contains(key),
                        open,
                        None,
                        None,
                        cx,
                    );
                    v_flex().child(header).when(open, |col| {
                        col.children(
                            list.iter()
                                .map(|task| instance_row(actions, task, false, cx)),
                        )
                    })
                }))
        })
}

fn build_menu_body(
    actions: &MenuActions,
    builds: &[(usize, Build)],
    runs: &[crate::config::RunConfig],
    cx: &mut App,
) -> impl IntoElement + use<> {
    let rows: usize = builds
        .iter()
        .map(|(_, b)| 1 + b.tasks.len() + runs.iter().filter(|r| r.root == b.root).count())
        .sum();
    menu_body(rows)
        .overflow_y_scrollbar()
        .children(builds.iter().map(|(b, build)| {
            let b = *b;
            let heading = build
                .root
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let on_add = actions.act(Box::new(move |this, window, cx| {
                this.add_run_config(b, window, cx)
            }));
            v_flex()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(menu_heading(heading, cx))
                        .child(
                            Button::new(SharedString::from(format!("java-config-add-{b}")))
                                .ghost()
                                .cursor_pointer()
                                .xsmall()
                                .icon(LucideIcon::Plus)
                                .tooltip("New run configuration")
                                .on_click(move |e, window, cx| on_add(e, window, cx)),
                        ),
                )
                .children(build.tasks.iter().enumerate().map(|(t, task)| {
                    let on_run = actions.act(Box::new(move |this, window, cx| {
                        this.run_java_task(b, t, window, cx)
                    }));
                    menu_row(SharedString::from(format!("java-task-{b}-{t}")), cx)
                        .on_click(move |e, window, cx| on_run(e, window, cx))
                        .child(task.label.clone())
                        .test_support()
                }))
                .children(
                    runs.iter()
                        .enumerate()
                        .filter(|(_, r)| r.root == build.root)
                        .map(|(i, run)| {
                            let on_run = actions.act(Box::new(move |this, window, cx| {
                                this.run_java_config(i, window, cx)
                            }));
                            let on_del = actions.keep_open(Box::new(move |this, _, cx| {
                                this.remove_run_config(i, cx)
                            }));
                            menu_row(SharedString::from(format!("java-config-{i}")), cx)
                                .on_click(move |e, window, cx| on_run(e, window, cx))
                                .child(Icon::new(LucideIcon::Play).small())
                                .child(div().flex_1().min_w_0().truncate().child(run.name.clone()))
                                .child(
                                    Button::new(SharedString::from(format!("java-config-del-{i}")))
                                        .ghost()
                                        .cursor_pointer()
                                        .xsmall()
                                        .icon(LucideIcon::Trash)
                                        .tooltip("Delete configuration")
                                        .on_click(move |e, window, cx| {
                                            cx.stop_propagation();
                                            on_del(e, window, cx)
                                        }),
                                )
                                .test_support()
                        }),
                )
        }))
}
