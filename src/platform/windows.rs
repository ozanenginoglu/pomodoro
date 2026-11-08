//! Windows: widget, görev çubuğunda görünmeyen, her zaman üstte duran bir
//! araç penceresidir (`WindowKind::PopUp` → `WS_EX_TOOLWINDOW | WS_EX_TOPMOST`).
//!
//! - Pencereye daire biçiminde bir bölge (`SetWindowRgn`) verilir: köşelerdeki
//!   transparan alana yapılan tıklamalar alttaki pencereye geçer ve fare
//!   daireden çıkınca Windows "fare çıktı" olayını gönderir.
//! - Dairenin boş alanı yerel sürükleme alanıdır (başlık çubuğu gibi); bırakılan
//!   konum widget tarafından ayarlara kaydedilir.

use gpui::{
    App, Bounds, Entity, Pixels, Point, Render, Window, WindowBounds, WindowHandle,
    WindowKind,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{ClientToScreen, CreateEllipticRgn, SetWindowRgn},
    UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOSIZE,
        SetWindowPos,
    },
};

use super::{Applied, settings_window_options, widget_bounds, widget_window_options};
use crate::config::AppearanceConfig;

pub fn open_widget<V: 'static + Render>(
    cx: &mut App,
    appearance: &AppearanceConfig,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>> {
    let bounds = widget_bounds(cx, appearance);
    let options = gpui::WindowOptions {
        kind: WindowKind::PopUp,
        ..widget_window_options(WindowBounds::Windowed(bounds))
    };
    let handle = cx.open_window(options, build)?;
    let appearance = appearance.clone();
    handle.update(cx, |_, window, cx| {
        apply_appearance(window, cx, &appearance);
    })?;
    Ok(handle)
}

/// Ayarlar paneli de üstte duran bir araç penceresidir; aksi halde ekranın
/// ortasındaki widget paneli örterdi.
pub fn open_settings<V: 'static + Render>(
    cx: &mut App,
    bounds: Bounds<Pixels>,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>> {
    let options = gpui::WindowOptions {
        kind: WindowKind::PopUp,
        ..settings_window_options(WindowBounds::Windowed(bounds))
    };
    cx.open_window(options, build)
}

fn hwnd(window: &Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as _)),
        _ => None,
    }
}

/// İstemci alanının (GPUI'nin çizdiği alan) pencere dikdörtgeni içindeki
/// konumu ve boyutu, fiziksel piksel. Başlık çubuğu gizlendiğinde GPUI yalnızca
/// üst kenarı kaldırır; sol, sağ ve alttaki görünmez boyutlandırma kenarlıkları
/// pencere dikdörtgeninde kalır. Hem bölge hem konum bu farka göre ayarlanmalı.
fn client_area(hwnd: HWND) -> Option<(POINT, RECT)> {
    unsafe {
        let mut window = RECT::default();
        let mut client = RECT::default();
        GetWindowRect(hwnd, &mut window).ok()?;
        GetClientRect(hwnd, &mut client).ok()?;
        let mut origin = POINT::default();
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
        Some((POINT { x: origin.x - window.left, y: origin.y - window.top }, client))
    }
}

/// Konum, boyut ve "üstte dur" ayarını uygular; daire bölgesini yeniler.
pub fn apply_appearance(window: &mut Window, cx: &App, appearance: &AppearanceConfig) -> Applied {
    let Some(hwnd) = hwnd(window) else { return Applied::Now };
    let scale = window.scale_factor();
    let (offset, client) = client_area(hwnd).unwrap_or_default();
    // Daire istemci alanındaki karenin ortasında çizilir (bkz. `ring`).
    let side = (appearance.size.pixels() * scale).round() as i32;
    let (width, height) = (client.right - client.left, client.bottom - client.top);
    let side = if width > 0 && height > 0 { side.min(width).min(height) } else { side };
    let left = offset.x + (width.max(side) - side) / 2;
    let top = offset.y + (height.max(side) - side) / 2;

    // Ayarlardaki konum istemci alanının sol üst köşesidir.
    let origin = widget_bounds(cx, appearance).origin;
    let x = (f32::from(origin.x) * scale).round() as i32 - offset.x;
    let y = (f32::from(origin.y) * scale).round() as i32 - offset.y;
    let order = if appearance.always_on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe {
        if let Err(e) = SetWindowPos(hwnd, Some(order), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE) {
            log::warn!("pencere konumlanamadı: {e}");
        }
        // Kenar yumuşatması kırpılmasın diye bölge bir piksel geniş.
        // Bölgenin sahibi artık sistemdir; serbest bırakmıyoruz.
        let region = CreateEllipticRgn(left, top, left + side + 1, top + side + 1);
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            log::warn!("daire bölgesi uygulanamadı");
        }
    }
    log::debug!("pencere: istemci ofseti {offset:?}, istemci {client:?}, daire {side}px");
    Applied::Now
}

/// Kullanıcı widget'ı sürükleyince konumu hatırlanır.
pub fn remembers_position(_cx: &App) -> bool {
    true
}

/// Dairenin boş alanı pencereyi sürükler (Windows'ta pencere yöneticisi
/// kısayolu yok).
pub const DRAG_TO_MOVE: bool = true;

pub fn is_xwayland() -> bool {
    false
}

/// Windows'ta fare olayları güvenilir; daire bölgesi sayesinde "fare çıktı"
/// olayı dairenin kenarında gelir. Yoklamaya gerek yok.
pub fn pointer_position(_window: &Window) -> Option<Point<Pixels>> {
    None
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Pomodoro";

/// Oturum açılınca başlıyor mu? (`HKCU\...\Run`)
pub fn autostart() -> Option<bool> {
    let key = windows_registry::CURRENT_USER.open(RUN_KEY).ok()?;
    Some(key.get_string(RUN_VALUE).is_ok())
}

pub fn set_autostart(enabled: bool) -> anyhow::Result<()> {
    let key = windows_registry::CURRENT_USER.create(RUN_KEY)?;
    if enabled {
        let exe = std::env::current_exe()?;
        key.set_string(RUN_VALUE, format!("\"{}\"", exe.display()))?;
    } else {
        key.remove_value(RUN_VALUE)?;
    }
    Ok(())
}
