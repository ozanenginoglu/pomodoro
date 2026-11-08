//! Windows özel kodu. Uygulama içi sürükleme, konum hafızası ve named
//! pipe ile `toggle` komutu Aşama 7'de gelecek.

use gpui::{
    App, AppContext, Bounds, Entity, Pixels, Point, Render, Window, WindowBounds, WindowHandle,
};

use super::{Applied, settings_window_options, widget_bounds, widget_window_options};
use crate::config::AppearanceConfig;

pub fn open_widget<V: 'static + Render>(
    cx: &mut App,
    appearance: &AppearanceConfig,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>> {
    let bounds = widget_bounds(cx, appearance);
    cx.open_window(widget_window_options(WindowBounds::Windowed(bounds)), build)
}

pub fn open_settings<V: 'static + Render>(
    cx: &mut App,
    bounds: Bounds<Pixels>,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>> {
    cx.open_window(settings_window_options(WindowBounds::Windowed(bounds)), build)
}

/// Aşama 7'de doldurulacak.
pub fn apply_appearance(
    _window: &mut Window,
    _cx: &App,
    _appearance: &AppearanceConfig,
) -> Applied {
    Applied::Now
}

pub fn is_xwayland() -> bool {
    false
}

/// Aşama 7'de `GetCursorPos` + `ScreenToClient` ile doldurulacak. Şimdilik
/// GPUI'nin kendi hover olaylarına güveniyoruz.
pub fn pointer_position(_window: &Window) -> Option<Point<Pixels>> {
    None
}
