//! Windows özel kodu. Uygulama içi sürükleme, konum hafızası ve named
//! pipe ile `toggle` komutu Aşama 7'de gelecek.

use gpui::{Pixels, Point, Window};

use super::StatusInfo;

/// Aşama 7'de `GetCursorPos` + `ScreenToClient` ile doldurulacak. Şimdilik
/// GPUI'nin kendi hover olaylarına güveniyoruz.
pub fn pointer_position(_window: &Window) -> Option<Point<Pixels>> {
    None
}

/// Windows'ta xmobar yok; düz metin durum satırı.
pub fn format_status(info: &StatusInfo) -> String {
    match info {
        StatusInfo::Idle => String::new(),
        StatusInfo::Pomodoro { remaining, paused, .. } => {
            format!("{remaining}{}", if *paused { " (duraklatıldı)" } else { "" })
        }
        StatusInfo::Meeting { remaining, ends_at, .. } => format!("{remaining} → {ends_at}"),
    }
}
