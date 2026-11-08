//! Linux: X11 ve Wayland.
//!
//! - **Wayland + layer-shell** (Hyprland, Sway, niri, KDE): widget gerçek bir
//!   masaüstü widget'ı olur; köşeye sabitlenir, görev çubuğunda görünmez.
//! - **Wayland, layer-shell yok** (GNOME): normal pencere; konumu compositor seçer.
//! - **X11 / XWayland**: konum açılışta verilir; "üstte dur" ve "görev
//!   çubuğunda gösterme" EWMH ipuçlarıyla istenir.

use std::cell::RefCell;

use gpui::{
    App, Bounds, Entity, Pixels, Point, Render, Window, WindowBounds, WindowHandle, WindowKind,
    layer_shell::{
        Anchor, KeyboardInteractivity, Layer, LayerShellNotSupportedError, LayerShellOptions,
    },
    point, px, size,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::{
    connection::Connection,
    protocol::xproto::{ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask},
    rust_connection::RustConnection,
};

use super::{
    APP_ID, Applied, SETTINGS_APP_ID, settings_window_options, widget_bounds, widget_window_options,
};
use crate::config::{AppearanceConfig, Position};

thread_local! {
    /// İmleç sorguları ve EWMH istekleri için ayrı, tembel açılan bir X bağlantısı.
    static X11: RefCell<Option<RustConnection>> = const { RefCell::new(None) };
}

fn is_wayland(cx: &App) -> bool {
    cx.compositor_name() == "Wayland"
}

/// Widget penceresini açar. `build` başarısız bir denemeden sonra tekrar
/// çağrılabileceği için `Fn`.
pub fn open_widget<V: 'static + Render>(
    cx: &mut App,
    appearance: &AppearanceConfig,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>> {
    let side = px(appearance.size.pixels());
    if is_wayland(cx) {
        let options = gpui::WindowOptions {
            kind: WindowKind::LayerShell(layer_shell_options(appearance)),
            ..widget_window_options(WindowBounds::Windowed(Bounds::centered(
                None,
                size(side, side),
                cx,
            )))
        };
        match cx.open_window(options, &build) {
            Ok(handle) => return Ok(handle),
            Err(e)
                if e.chain().any(|c| c.downcast_ref::<LayerShellNotSupportedError>().is_some()) =>
            {
                log::info!("compositor layer-shell desteklemiyor; normal pencere açılıyor");
            }
            Err(e) => return Err(e),
        }
        // Wayland istemcileri kendi konumlarını seçemez; yeri compositor belirler.
        let bounds = Bounds::centered(None, size(side, side), cx);
        return cx.open_window(widget_window_options(WindowBounds::Windowed(bounds)), &build);
    }

    let bounds = widget_bounds(cx, appearance);
    let handle = cx.open_window(widget_window_options(WindowBounds::Windowed(bounds)), &build)?;
    let always_on_top = appearance.always_on_top;
    handle.update(cx, |_, window, _| set_x11_state(window, always_on_top))?;
    Ok(handle)
}

/// Ayarlar panelini açar. Wayland'de widget bir layer-shell yüzeyi olduğundan
/// panelin "ebeveyni" olamaz; döşemeli compositor'lar (niri, Hyprland) onu
/// tam ekran bir döşemeye yerleştirir. Bu yüzden panel de ekranın ortasında
/// bir layer-shell yüzeyi olarak açılır.
pub fn open_settings<V: 'static + Render>(
    cx: &mut App,
    bounds: Bounds<Pixels>,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>> {
    if is_wayland(cx) {
        let options = gpui::WindowOptions {
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: SETTINGS_APP_ID.to_string(),
                layer: Layer::Overlay,
                anchor: Anchor::empty(),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                ..Default::default()
            }),
            ..settings_window_options(WindowBounds::Windowed(bounds))
        };
        match cx.open_window(options, &build) {
            Ok(handle) => return Ok(handle),
            Err(e)
                if e.chain().any(|c| c.downcast_ref::<LayerShellNotSupportedError>().is_some()) => {
            }
            Err(e) => return Err(e),
        }
    }
    cx.open_window(settings_window_options(WindowBounds::Windowed(bounds)), &build)
}

fn layer_shell_options(appearance: &AppearanceConfig) -> LayerShellOptions {
    let anchor = match appearance.position {
        Position::Center => Anchor::empty(),
        Position::TopLeft => Anchor::TOP | Anchor::LEFT,
        Position::TopRight => Anchor::TOP | Anchor::RIGHT,
        Position::BottomLeft => Anchor::BOTTOM | Anchor::LEFT,
        Position::BottomRight => Anchor::BOTTOM | Anchor::RIGHT,
    };
    let m = px(appearance.margin as f32);
    LayerShellOptions {
        namespace: APP_ID.to_string(),
        // "Üstte dur" kapalıyken pencerelerin altında, masaüstünde durur.
        layer: if appearance.always_on_top { Layer::Top } else { Layer::Bottom },
        anchor,
        exclusive_zone: None,
        exclusive_edge: None,
        margin: Some((m, m, m, m)),
        // Toplantı süresi yazılabilsin diye tıklanınca klavye odağı alır.
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
    }
}

/// Görünüm ayarları değişince çağrılır.
pub fn apply_appearance(window: &mut Window, cx: &App, appearance: &AppearanceConfig) -> Applied {
    if is_wayland(cx) {
        return Applied::NeedsRestart;
    }
    let origin = widget_bounds(cx, appearance).origin;
    move_x11_window(window, origin);
    set_x11_state(window, appearance.always_on_top);
    Applied::Now
}

fn x11_window_id(window: &Window) -> Option<u32> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        _ => None,
    }
}

fn with_x11<R>(f: impl FnOnce(&RustConnection) -> Option<R>) -> Option<R> {
    X11.with_borrow_mut(|conn| {
        if conn.is_none() {
            *conn = RustConnection::connect(None).ok().map(|(c, _)| c);
        }
        let result = f(conn.as_ref()?);
        if result.is_none() {
            // Bağlantı koptuysa bir sonraki çağrıda yeniden açılsın.
            *conn = None;
        }
        result
    })
}

fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
    Some(conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom)
}

/// EWMH `_NET_WM_STATE` isteği: üstte dur, görev çubuğunda / sayfalayıcıda
/// gösterme. Pencere yöneticisi desteklemiyorsa (ör. xmonad'ın "above"u) yok sayılır.
fn set_x11_state(window: &Window, above: bool) {
    let Some(xid) = x11_window_id(window) else {
        return;
    };
    with_x11(|conn| {
        let root = conn.setup().roots.first()?.root;
        let state = atom(conn, "_NET_WM_STATE")?;
        let send = |add: bool, a: u32, b: u32| -> Option<()> {
            let data = [u32::from(add), a, b, 1, 0];
            let event = ClientMessageEvent::new(32, xid, state, data);
            conn.send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .ok()?;
            Some(())
        };
        send(
            true,
            atom(conn, "_NET_WM_STATE_SKIP_TASKBAR")?,
            atom(conn, "_NET_WM_STATE_SKIP_PAGER")?,
        )?;
        send(above, atom(conn, "_NET_WM_STATE_ABOVE")?, 0)?;
        conn.flush().ok()
    });
}

fn move_x11_window(window: &Window, origin: Point<Pixels>) {
    let Some(xid) = x11_window_id(window) else {
        return;
    };
    let scale = window.scale_factor();
    let (x, y) = ((f32::from(origin.x) * scale) as i32, (f32::from(origin.y) * scale) as i32);
    with_x11(|conn| {
        conn.configure_window(xid, &ConfigureWindowAux::new().x(x).y(y)).ok()?;
        conn.flush().ok()
    });
}

/// X sunucusu bir Wayland oturumundaki XWayland mı? (`XWAYLAND` uzantısı)
pub fn is_xwayland() -> bool {
    thread_local! {
        static XWAYLAND: bool = with_x11(|conn| {
            Some(conn.query_extension(b"XWAYLAND").ok()?.reply().ok()?.present)
        })
        .unwrap_or(false);
    }
    XWAYLAND.with(|x| *x)
}

/// İmlecin pencereye göre konumu (mantıksal piksel). Yalnızca X11'de; GPUI'nin
/// X11 "leave" olayı xmonad altında güvenilir gelmediği için doğrudan sorguluyoruz.
/// Wayland'de `None`: orada imleç olayları güvenilir, yoklamaya gerek yok.
pub fn pointer_position(window: &Window) -> Option<Point<Pixels>> {
    let xid = x11_window_id(window)?;
    let scale = window.scale_factor();
    with_x11(|conn| {
        let reply = conn.query_pointer(xid).ok()?.reply().ok()?;
        Some(
            reply
                .same_screen
                .then(|| point(px(reply.win_x as f32 / scale), px(reply.win_y as f32 / scale))),
        )
    })
    .flatten()
}
