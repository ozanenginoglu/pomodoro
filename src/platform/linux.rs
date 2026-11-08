//! Linux: X11 ve Wayland.
//!
//! - **Wayland + layer-shell** (Hyprland, Sway, niri, KDE): widget gerçek bir
//!   masaüstü widget'ı olur; köşeye sabitlenir, görev çubuğunda görünmez.
//! - **Wayland, layer-shell yok** (GNOME): normal pencere; konumu compositor seçer.
//! - **X11 / XWayland**: konum açılışta verilir; "üstte dur" ve "görev
//!   çubuğunda gösterme" EWMH ipuçlarıyla istenir.

use std::{
    cell::RefCell,
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

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
    protocol::{
        screensaver::ConnectionExt as _,
        xproto::{ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask},
    },
    rust_connection::RustConnection,
};

use super::{
    ACTIVE_WINDOW, APP_ID, Applied, SETTINGS_APP_ID, settings_window_options, widget_bounds,
    widget_window_options,
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
    let m = px(appearance.margin as f32);
    let (anchor, margin) = match (appearance.position, appearance.custom) {
        (Position::Center, _) => (Anchor::empty(), (m, m, m, m)),
        (Position::TopLeft, _) => (Anchor::TOP | Anchor::LEFT, (m, m, m, m)),
        (Position::TopRight, _) => (Anchor::TOP | Anchor::RIGHT, (m, m, m, m)),
        (Position::BottomLeft, _) => (Anchor::BOTTOM | Anchor::LEFT, (m, m, m, m)),
        (Position::BottomRight, _) => (Anchor::BOTTOM | Anchor::RIGHT, (m, m, m, m)),
        // Sol üst köşeden uzaklık olarak (çoklu ekranda birincil ekrana göre).
        (Position::Custom, Some([x, y])) => (
            Anchor::TOP | Anchor::LEFT,
            (px(y.max(0) as f32), px(0.0), px(0.0), px(x.max(0) as f32)),
        ),
        (Position::Custom, None) => (Anchor::empty(), (m, m, m, m)),
    };
    LayerShellOptions {
        namespace: APP_ID.to_string(),
        // "Üstte dur" kapalıyken pencerelerin altında, masaüstünde durur.
        layer: if appearance.always_on_top { Layer::Top } else { Layer::Bottom },
        anchor,
        exclusive_zone: None,
        exclusive_edge: None,
        margin: Some(margin),
        // Toplantı süresi yazılabilsin diye tıklanınca klavye odağı alır.
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
    }
}

/// Pencere yöneticisiyle taşınan konum hatırlanabilir mi? Wayland'de
/// istemci kendi konumunu bilemez.
pub fn remembers_position(cx: &App) -> bool {
    !is_wayland(cx)
}

/// Widget'ın arka planı sürükleme alanı mı? Linux'ta pencere yöneticisi
/// taşır (xmonad: mod + sürükle); tıklamalar widget'a kalır.
pub const DRAG_TO_MOVE: bool = false;

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

/// Gözün imleci izlemesi için imlecin pencereye göre konumu; pencerenin
/// dışında da (X11). Wayland'de imleç ancak pencerenin üstündeyken bilinir.
pub fn cursor_position(window: &Window) -> Option<Point<Pixels>> {
    pointer_position(window)
}

/// Kullanıcı son `ACTIVE_WINDOW` içinde klavye ya da fare kullandı mı?
/// Bilinemiyorsa (ör. GNOME Wayland) `None`.
///
/// Wayland oturumunda (XWayland dahil) `ext-idle-notify-v1` kullanılır; X
/// sunucusu yalnızca X istemcilerine giden girdiyi görür. X11'de
/// MIT-SCREEN-SAVER uzantısı.
pub fn user_active() -> Option<bool> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return wayland_idle::active();
    }
    with_x11(|conn| {
        let root = conn.setup().roots.first()?.root;
        let info = conn.screensaver_query_info(root).ok()?.reply().ok()?;
        Some(u128::from(info.ms_since_user_input) < ACTIVE_WINDOW.as_millis())
    })
}

/// `ext-idle-notify-v1` ile hareketsizlik takibi. GPUI'nin bağlantısından
/// bağımsız, kendi iş parçacığında çalışan ikinci bir Wayland bağlantısı.
mod wayland_idle {
    use super::*;
    use wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle,
        globals::{GlobalListContents, registry_queue_init},
        protocol::{wl_registry, wl_seat::WlSeat},
    };
    use wayland_protocols::ext::idle_notify::v1::client::{
        ext_idle_notification_v1::{self, ExtIdleNotificationV1},
        ext_idle_notifier_v1::ExtIdleNotifierV1,
    };

    const STARTING: u8 = 0;
    const ACTIVE: u8 = 1;
    const IDLE: u8 = 2;
    const UNSUPPORTED: u8 = 3;

    static STATE: AtomicU8 = AtomicU8::new(STARTING);
    static STARTED: OnceLock<()> = OnceLock::new();

    pub fn active() -> Option<bool> {
        STARTED.get_or_init(|| {
            let spawned = std::thread::Builder::new().name("idle-notify".into()).spawn(|| {
                if let Err(e) = run() {
                    log::info!("Wayland hareketsizlik bildirimi yok: {e}");
                }
                STATE.store(UNSUPPORTED, Ordering::Relaxed);
            });
            if spawned.is_err() {
                STATE.store(UNSUPPORTED, Ordering::Relaxed);
            }
        });
        match STATE.load(Ordering::Relaxed) {
            // Bildirim gelene kadar kullanıcı bilgisayar başında sayılır.
            STARTING | ACTIVE => Some(true),
            IDLE => Some(false),
            _ => None,
        }
    }

    struct State;

    fn run() -> anyhow::Result<()> {
        let conn = Connection::connect_to_env()?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=1, ())?;
        let notifier: ExtIdleNotifierV1 = globals.bind(&qh, 1..=2, ())?;
        let timeout = ACTIVE_WINDOW.as_millis() as u32;
        // Sürüm 2: video oynatıcıların "ekranı karartma" isteklerini yok sayar;
        // yalnızca gerçek klavye/fare girdisine bakar.
        if notifier.version() >= 2 {
            notifier.get_input_idle_notification(timeout, &seat, &qh, ());
        } else {
            notifier.get_idle_notification(timeout, &seat, &qh, ());
        }
        loop {
            queue.blocking_dispatch(&mut State)?;
        }
    }

    impl Dispatch<ExtIdleNotificationV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ExtIdleNotificationV1,
            event: ext_idle_notification_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let state = match event {
                ext_idle_notification_v1::Event::Idled => IDLE,
                ext_idle_notification_v1::Event::Resumed => ACTIVE,
                _ => return,
            };
            log::debug!("wayland hareketsizlik: {}", state == IDLE);
            STATE.store(state, Ordering::Relaxed);
        }
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    wayland_client::delegate_noop!(State: ignore WlSeat);
    wayland_client::delegate_noop!(State: ExtIdleNotifierV1);
}

/// Linux'ta açılışta başlatma pencere yöneticisinin işi (xmonad `spawnOnce`,
/// Hyprland `exec-once` …); ayarlarda gösterilmez.
pub fn autostart() -> Option<bool> {
    None
}

pub fn set_autostart(_enabled: bool) -> anyhow::Result<()> {
    anyhow::bail!("Linux'ta desteklenmiyor")
}
