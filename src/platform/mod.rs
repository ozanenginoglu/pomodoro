//! Platforma özel davranışlar. Geri kalan kod bu modülün arkasındaki
//! ortak arayüzü kullanır; Linux / Windows farkları yalnızca burada yaşar.

use std::time::Duration;

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
/// Ayarlar panelinin köşe yarıçapı (mantıksal piksel). Windows'ta pencere
/// bölgesi de buna göre kırpılır.
pub const SETTINGS_RADIUS: f32 = 16.0;

/// Mola koruması: son klavye/fare kullanımından bu kadar süre geçmediyse
/// kullanıcı bilgisayar başında sayılır.
pub const ACTIVE_WINDOW: Duration = Duration::from_secs(10);

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
    let displays: Vec<_> = cx.displays().iter().map(|d| d.bounds()).collect();
    // Sürüklenmiş konum hangi ekrandaysa o; o ekran artık yoksa birincil ekran.
    let custom_display =
        appearance.custom.filter(|_| appearance.position == Position::Custom).and_then(|[x, y]| {
            let center = point(px(x as f32) + side / 2.0, px(y as f32) + side / 2.0);
            displays.iter().copied().find(|d| d.contains(&center))
        });
    let Some(display) = custom_display.or_else(|| cx.primary_display().map(|d| d.bounds())) else {
        return Bounds::centered(None, size(side, side), cx);
    };
    let mut appearance = appearance.clone();
    if custom_display.is_none() && appearance.position == Position::Custom {
        appearance.position = Position::Center;
    }
    Bounds { origin: widget_origin(display, side, &appearance), size: size(side, side) }
}

/// Pencere, ayarlardaki konumdan kullanıcı tarafından mı taşındı? Pencere
/// yöneticilerinin birkaç piksellik düzeltmeleri sayılmaz.
pub fn moved_by_user(expected: Point<Pixels>, actual: Point<Pixels>) -> bool {
    const TOLERANCE: f32 = 8.0;
    (f32::from(expected.x) - f32::from(actual.x)).abs() > TOLERANCE
        || (f32::from(expected.y) - f32::from(actual.y)).abs() > TOLERANCE
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
        // Ekranın dışına taşmasın (çözünürlük değişmiş olabilir).
        Position::Custom => {
            let [x, y] = appearance.custom.unwrap_or_default();
            let max_x = display.origin.x + display.size.width - side;
            let max_y = display.origin.y + display.size.height - side;
            point(
                px(x as f32).clamp(display.origin.x, max_x.max(display.origin.x)),
                px(y as f32).clamp(display.origin.y, max_y.max(display.origin.y)),
            )
        }
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

    #[test]
    fn custom_position_is_kept_on_screen() {
        let display =
            Bounds { origin: point(px(0.0), px(0.0)), size: size(px(1920.0), px(1080.0)) };
        let at = |custom| {
            widget_origin(
                display,
                px(200.0),
                &AppearanceConfig {
                    position: Position::Custom,
                    custom: Some(custom),
                    ..Default::default()
                },
            )
        };
        assert_eq!(at([300, 400]), point(px(300.0), px(400.0)));
        assert_eq!(at([1900, -50]), point(px(1720.0), px(0.0)));
    }

    #[test]
    fn small_adjustments_are_not_user_moves() {
        let a = point(px(776.0), px(24.0));
        assert!(!moved_by_user(a, point(px(778.0), px(24.0))));
        assert!(moved_by_user(a, point(px(500.0), px(300.0))));
    }
}
