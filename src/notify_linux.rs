//! Linux notification with a click that really raises the window.
//!
//! GNOME / KDE only let an app focus itself with a one-time activation token,
//! and the notification daemon hands that token to the app whose notification
//! was clicked (`ActivationToken` signal, just before `ActionInvoked`). So we
//! post the notification over D-Bus ourselves, wait for the click, and give
//! the token to the compositor via `xdg_activation_v1`. `notify-send` cannot
//! report the token.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::gpui::Window;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
use zbus::blocking::{Connection as Bus, Proxy as BusProxy};
use zbus::zvariant::Value;

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const APP_ICON: &str = "app.isengard.edtir";

/// Id of the notification currently on screen / in the tray; a new alert
/// replaces it so the tray does not pile up, and the old listener retires.
static LAST_ID: AtomicU32 = AtomicU32::new(0);
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// Post `title` / `body`; when the user clicks it, call `on_click` with the
/// activation token (if the daemon sent one). Blocks until the notification is
/// clicked, closed or replaced — run it on its own thread. `false` when the
/// notification could not be shown over D-Bus (caller falls back).
pub fn show(title: &str, body: &str, on_click: impl FnOnce(Option<String>)) -> bool {
    match run(title, body, on_click) {
        Ok(()) => true,
        Err(err) => {
            log::warn!("D-Bus notification failed: {err}");
            false
        }
    }
}

fn run(
    title: &str,
    body: &str,
    on_click: impl FnOnce(Option<String>),
) -> zbus::Result<()> {
    let bus = Bus::session()?;
    let proxy = BusProxy::new(&bus, DEST, PATH, DEST)?;
    // Subscribe first so the token signal cannot slip past.
    let signals = proxy.receive_all_signals()?;
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let hints: HashMap<&str, Value> = HashMap::new();
    let id: u32 = proxy.call(
        "Notify",
        &(
            "Isengard",
            LAST_ID.load(Ordering::SeqCst),
            APP_ICON,
            title,
            body,
            ["default", "Open"].as_slice(),
            hints,
            -1i32,
        ),
    )?;
    LAST_ID.store(id, Ordering::SeqCst);

    let mut token = None;
    for msg in signals {
        if GENERATION.load(Ordering::SeqCst) != generation {
            return Ok(()); // replaced by a newer alert
        }
        let header = msg.header();
        let member = header.member().map(|m| m.as_str().to_owned());
        match member.as_deref() {
            Some("ActivationToken") => {
                if let Ok((nid, t)) = msg.body().deserialize::<(u32, String)>()
                    && nid == id
                {
                    token = Some(t);
                }
            }
            Some("ActionInvoked") => {
                if let Ok((nid, key)) = msg.body().deserialize::<(u32, String)>()
                    && nid == id
                    && key == "default"
                {
                    on_click(token);
                    return Ok(());
                }
            }
            Some("NotificationClosed") => {
                if let Ok((nid, _)) = msg.body().deserialize::<(u32, u32)>()
                    && nid == id
                {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Raise `window` with `token` on Wayland, else fall back to GPUI's request
/// (fine on X11; on Wayland without a token it only flags the window).
/// UI thread only.
pub fn raise(window: &Window, token: Option<&str>) {
    if let Some(token) = token
        && activate_wayland(window, token).is_some()
    {
        return;
    }
    window.activate_window();
}

fn activate_wayland(window: &Window, token: &str) -> Option<()> {
    let RawWindowHandle::Wayland(w) = HasWindowHandle::window_handle(window).ok()?.as_raw() else {
        return None;
    };
    let RawDisplayHandle::Wayland(d) = HasDisplayHandle::display_handle(window).ok()?.as_raw() else {
        return None;
    };
    // SAFETY: both pointers come from GPUI's live Wayland window on this (UI)
    // thread; the foreign backend only borrows the display, it never closes it.
    unsafe {
        let backend = Backend::from_foreign_display(d.display.as_ptr().cast());
        let conn = Connection::from_backend(backend);
        let id = ObjectId::from_ptr(WlSurface::interface(), w.surface.as_ptr().cast()).ok()?;
        let surface = WlSurface::from_id(&conn, id).ok()?;
        let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
        let activation: XdgActivationV1 = globals.bind(&queue.handle(), 1..=1, ()).ok()?;
        activation.activate(token.to_owned(), &surface);
        conn.flush().ok()?;
    }
    Some(())
}
