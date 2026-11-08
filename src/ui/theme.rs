//! Dracula tabanlı renkler (xmobar ile uyumlu).

use gpui::{Hsla, rgb};

use crate::{meeting::Stage, pomodoro::Phase};

pub fn surface() -> Hsla {
    rgb(0x282a36).into()
}
pub fn ring_track() -> Hsla {
    rgb(0x3a3c4e).into()
}
pub fn text() -> Hsla {
    rgb(0xf8f8f2).into()
}
pub fn muted() -> Hsla {
    rgb(0x6272a4).into()
}
/// Açık anahtarlar gibi vurgu öğeleri.
pub fn accent() -> Hsla {
    rgb(0xbd93f9).into()
}
pub fn button_hover() -> Hsla {
    rgb(0x44475a).into()
}

/// Her evrenin kendi rengi: halka, tur noktaları ve başlat düğmesi bu renkte.
pub fn phase(phase: Phase) -> Hsla {
    match phase {
        Phase::Work => rgb(0xff79c6).into(),       // pembe
        Phase::ShortBreak => rgb(0x50fa7b).into(), // yeşil
        Phase::LongBreak => rgb(0x8be9fd).into(),  // camgöbeği
    }
}

/// Toplantı aşamasına göre renk: mor → sarı → turuncu → kırmızı.
pub fn stage(stage: Stage) -> Hsla {
    match stage {
        Stage::Normal => rgb(0xbd93f9).into(),   // mor
        Stage::Warning => rgb(0xf1fa8c).into(),  // sarı
        Stage::Critical => rgb(0xffb86c).into(), // turuncu
        Stage::Overtime => rgb(0xff5555).into(), // kırmızı
    }
}

pub fn error() -> Hsla {
    rgb(0xff5555).into()
}

/// "#rrggbb" biçiminde renk (xmobar için).
pub fn hex(color: Hsla) -> String {
    let c = color.to_rgb();
    let byte = |v: f32| (v * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}
