//! Platforma özel davranışlar. Geri kalan kod bu modülün arkasındaki
//! ortak arayüzü kullanır; Linux / Windows farkları yalnızca burada yaşar.

use gpui::{WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind, WindowOptions};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// Durum satırının platformdan bağımsız içeriği.
pub enum StatusInfo {
    /// Widget açık ama hiçbir sayaç çalışmıyor.
    Idle,
    Pomodoro { remaining: String, on_break: bool, paused: bool, color: String },
    Meeting { remaining: String, ends_at: String, color: String },
}

/// Pencere sınıfı (X11'de WM_CLASS). xmonad/picom kuralları bu ada bakar.
pub const APP_ID: &str = "pomodoro";
pub const SETTINGS_APP_ID: &str = "pomodoro-settings";

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
