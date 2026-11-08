//! Platforma özel davranışlar. Geri kalan kod bu modülün arkasındaki
//! ortak arayüzü kullanır; Linux / Windows farkları yalnızca burada yaşar.

use gpui::{
    App, Bounds, Pixels, Point, WindowBackgroundAppearance, WindowBounds, WindowDecorations,
    WindowKind, WindowOptions, point, px, size,
};

use crate::config::{AppearanceConfig, Position};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// Pencere sınıfı (X11'de WM_CLASS, Wayland'de app_id / layer namespace).
/// xmonad/picom/Hyprland kuralları bu ada bakar.
pub const APP_ID: &str = "pomodoro";
pub const SETTINGS_APP_ID: &str = "pomodoro-settings";

/// Bir görünüm ayarı değişikliğinin nasıl uygulandığı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Now,
    /// Platform pencereyi yerinde değiştiremiyor (Wayland layer-shell);
    /// uygulama yeniden başlatılmalı.
    NeedsRestart,
}

/// Başlık çubuğu olmayan, arka planı transparan widget penceresi.
pub fn widget_window_options(bounds: WindowBounds) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(bounds),
        titlebar: None,
        kind: WindowKind::Normal,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(APP_ID.to_string()),
        ..Default::default()
    }
}

/// Ayarlar paneli: başlık çubuğu yok, köşeleri yuvarlak (dışı transparan).
/// `Floating` X11'de widget'ın geçici penceresi (WM_TRANSIENT_FOR) olur;
/// xmonad bu tür pencereleri kendiliğinden yüzdürür. Windows'ta da widget'ın
/// üstünde kalan sahipli pencere olur.
pub fn settings_window_options(bounds: WindowBounds) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(bounds),
        titlebar: None,
        kind: WindowKind::Floating,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(SETTINGS_APP_ID.to_string()),
        ..Default::default()
    }
}

/// Widget'ın ekrandaki sınırları (mantıksal piksel), ayarlardaki konuma göre.
pub fn widget_bounds(cx: &App, appearance: &AppearanceConfig) -> Bounds<Pixels> {
    let side = px(appearance.size.pixels());
    let Some(display) = cx.primary_display().map(|d| d.bounds()) else {
        return Bounds::centered(None, size(side, side), cx);
    };
    Bounds { origin: widget_origin(display, side, appearance), size: size(side, side) }
}

fn widget_origin(
    display: Bounds<Pixels>,
    side: Pixels,
    appearance: &AppearanceConfig,
) -> Point<Pixels> {
    let margin = px(appearance.margin as f32);
    let (left, top) = (display.origin.x + margin, display.origin.y + margin);
    let right = display.origin.x + display.size.width - side - margin;
    let bottom = display.origin.y + display.size.height - side - margin;
    match appearance.position {
        Position::Center => point(
            display.origin.x + (display.size.width - side) / 2.0,
            display.origin.y + (display.size.height - side) / 2.0,
        ),
        Position::TopLeft => point(left, top),
        Position::TopRight => point(right, top),
        Position::BottomLeft => point(left, bottom),
        Position::BottomRight => point(right, bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_respect_margin_and_display_origin() {
        // İkinci monitör: (1920, 0) konumunda 2560x1440.
        let display =
            Bounds { origin: point(px(1920.0), px(0.0)), size: size(px(2560.0), px(1440.0)) };
        let side = px(200.0);
        let at = |position| {
            widget_origin(
                display,
                side,
                &AppearanceConfig { position, margin: 24, ..Default::default() },
            )
        };
        assert_eq!(at(Position::TopLeft), point(px(1944.0), px(24.0)));
        assert_eq!(at(Position::TopRight), point(px(4256.0), px(24.0)));
        assert_eq!(at(Position::BottomRight), point(px(4256.0), px(1216.0)));
        assert_eq!(at(Position::Center), point(px(3100.0), px(620.0)));
    }
}
