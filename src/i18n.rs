//! Arayüz dilleri: İngilizce (varsayılan) ve Türkçe.
//!
//! Etkin dil süreç genelinde tek bir değerdir; bildirimler, durum satırı ve
//! komut satırı gibi pencere dışındaki kodlar da `t()` ile metinlere ulaşır.
//! Ayarlardan dil değişince `set` çağrılır ve pencereler yeniden çizilir.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "tr")]
    Turkish,
}

impl Language {
    /// Dil seçicide görünen ad (her dil kendi adıyla).
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Turkish => "Türkçe",
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(language: Language) {
    CURRENT.store(language as u8, Ordering::Relaxed);
}

pub fn current() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::Turkish,
        _ => Language::English,
    }
}

/// Etkin dilin metinleri.
pub fn t() -> &'static Strings {
    match current() {
        Language::English => &EN,
        Language::Turkish => &TR,
    }
}

/// Arayüzdeki sabit metinler. Biçimlendirilen metinler aşağıdaki
/// fonksiyonlardadır.
pub struct Strings {
    // Ayarlar paneli
    pub settings: &'static str,
    pub section_pomodoro: &'static str,
    pub section_meeting: &'static str,
    pub section_notifications: &'static str,
    pub section_appearance: &'static str,
    pub work: &'static str,
    pub short_break: &'static str,
    pub long_break: &'static str,
    pub rounds_before_long_break: &'static str,
    pub auto_start_breaks: &'static str,
    pub auto_start_work: &'static str,
    pub display: &'static str,
    pub display_remaining: &'static str,
    pub display_end_time: &'static str,
    pub display_both: &'static str,
    pub warning: &'static str,
    pub critical: &'static str,
    pub notifications: &'static str,
    pub sound: &'static str,
    pub size: &'static str,
    pub position: &'static str,
    pub always_on_top: &'static str,
    pub start_on_sign_in: &'static str,
    pub language: &'static str,
    pub unit_minutes: &'static str,
    pub unit_rounds: &'static str,

    // Toplantı girişi
    pub meeting_input_hint: &'static str,
    pub error_empty: &'static str,
    pub error_invalid: &'static str,
    pub error_zero: &'static str,
    pub error_too_long: &'static str,

    // Bildirimler
    pub work_done: &'static str,
    pub break_done: &'static str,
    pub meeting: &'static str,
    pub meeting_over: &'static str,

    // Durum satırı (waybar ipucu, düz metin)
    pub status_work: &'static str,
    pub status_break: &'static str,
    pub status_paused: &'static str,

    // Komut satırı
    pub usage: &'static str,
    pub already_running: &'static str,
    pub not_running: &'static str,
}

static EN: Strings = Strings {
    settings: "Settings",
    section_pomodoro: "POMODORO",
    section_meeting: "MEETING",
    section_notifications: "NOTIFICATIONS",
    section_appearance: "APPEARANCE",
    work: "Work",
    short_break: "Short break",
    long_break: "Long break",
    rounds_before_long_break: "Long break after",
    auto_start_breaks: "Start breaks automatically",
    auto_start_work: "Start work automatically",
    display: "Show",
    display_remaining: "Left",
    display_end_time: "End",
    display_both: "Both",
    warning: "Warning",
    critical: "Final warning",
    notifications: "Notifications",
    sound: "Sound",
    size: "Size",
    position: "Position",
    always_on_top: "Always on top",
    start_on_sign_in: "Start on sign-in",
    language: "Language",
    unit_minutes: "min",
    unit_rounds: "rounds",

    meeting_input_hint: "length or end time  ↵",
    error_empty: "type a length or a time",
    error_invalid: "not understood",
    error_zero: "length can't be zero",
    error_too_long: "12 hours at most",

    work_done: "Work round done",
    break_done: "Break over",
    meeting: "Meeting",
    meeting_over: "Meeting time is up",

    status_work: "Work",
    status_break: "Break",
    status_paused: "paused",

    usage: "\
Usage: pomodoro [COMMAND]

Without a command, opens the widget (does nothing if it's already open).

Commands (sent to the running widget):
  toggle     Start / pause the Pomodoro timer
  reset      Restart the current round
  skip       Skip to the next round
  mode       Switch between Pomodoro and meeting mode
  settings   Open the settings panel
  status [--format xmobar|waybar|plain]
             Print the current state (xmobar by default on Linux;
             empty if the widget isn't running)
",
    already_running: "Pomodoro is already running.",
    not_running: "Pomodoro isn't running.",
};

static TR: Strings = Strings {
    settings: "Ayarlar",
    section_pomodoro: "POMODORO",
    section_meeting: "TOPLANTI",
    section_notifications: "BİLDİRİM",
    section_appearance: "GÖRÜNÜM",
    work: "Çalışma",
    short_break: "Kısa mola",
    long_break: "Uzun mola",
    rounds_before_long_break: "Uzun mola aralığı",
    auto_start_breaks: "Molayı kendiliğinden başlat",
    auto_start_work: "Çalışmayı kendiliğinden başlat",
    display: "Gösterim",
    display_remaining: "Kalan",
    display_end_time: "Bitiş",
    display_both: "İkisi",
    warning: "Uyarı",
    critical: "Son uyarı",
    notifications: "Bildirim",
    sound: "Ses",
    size: "Boyut",
    position: "Konum",
    always_on_top: "Her zaman üstte",
    start_on_sign_in: "Oturum açılınca başlat",
    language: "Dil",
    unit_minutes: "dk",
    unit_rounds: "tur",

    meeting_input_hint: "süre ya da saat  ↵",
    error_empty: "süre ya da saat yazın",
    error_invalid: "anlaşılamadı",
    error_zero: "süre sıfır olamaz",
    error_too_long: "en fazla 12 saat",

    work_done: "Çalışma bitti",
    break_done: "Mola bitti",
    meeting: "Toplantı",
    meeting_over: "Toplantı süresi doldu",

    status_work: "Çalışma",
    status_break: "Mola",
    status_paused: "duraklatıldı",

    usage: "\
Kullanım: pomodoro [KOMUT]

Komut verilmezse widget'ı açar (zaten açıksa bir şey yapmaz).

Komutlar (çalışan widget'a gönderilir):
  toggle     Pomodoro'yu başlat / duraklat
  reset      Mevcut evreyi baştan al
  skip       Sonraki evreye geç
  mode       Pomodoro ↔ toplantı
  settings   Ayarlar panelini aç
  status [--format xmobar|waybar|plain]
             Durumu yazdır (Linux'ta varsayılan xmobar;
             widget kapalıysa boş)
",
    already_running: "Pomodoro zaten çalışıyor.",
    not_running: "Pomodoro çalışmıyor.",
};

// ---- Biçimlendirilen metinler ----------------------------------------------

/// Pomodoro evresi bitince: sonraki evre kendiliğinden başladıysa.
pub fn phase_started(next: &str, minutes: u64) -> String {
    match current() {
        Language::English => format!("{next} started · {minutes} min"),
        Language::Turkish => format!("{next} başladı · {minutes} dk"),
    }
}

/// Pomodoro evresi bitince: sonraki evre başlatılmayı bekliyorsa.
pub fn phase_up_next(next: &str, minutes: u64) -> String {
    match current() {
        Language::English => format!("Up next: {} · {minutes} min", next.to_lowercase()),
        Language::Turkish => format!("Sırada: {} · {minutes} dk", next.to_lowercase()),
    }
}

pub fn meeting_minutes_left(minutes: u64, end: &str) -> String {
    match current() {
        Language::English if minutes == 1 => format!("1 minute left · ends {end}"),
        Language::English => format!("{minutes} minutes left · ends {end}"),
        Language::Turkish => format!("Bitişe {minutes} dakika kaldı · {end}"),
    }
}

pub fn meeting_was_due(end: &str) -> String {
    match current() {
        Language::English => format!("It was due to end at {end}"),
        Language::Turkish => format!("Bitiş saati {end} idi"),
    }
}

/// waybar ipucu: "Meeting · 12:00 · ends 14:30".
pub fn status_meeting(remaining: &str, end: &str) -> String {
    match current() {
        Language::English => format!("Meeting · {remaining} · ends {end}"),
        Language::Turkish => format!("Toplantı · {remaining} · bitiş {end}"),
    }
}

pub fn unknown_command(command: &str) -> String {
    match current() {
        Language::English => format!("unknown command: {command}"),
        Language::Turkish => format!("bilinmeyen komut: {command}"),
    }
}

pub fn unknown_format(format: &str) -> String {
    match current() {
        Language::English => format!("unknown format: {format} (xmobar, waybar, plain)"),
        Language::Turkish => format!("bilinmeyen biçim: {format} (xmobar, waybar, plain)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_is_stored_as_short_code() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            language: Language,
        }
        let text = toml::to_string(&Wrap { language: Language::Turkish }).unwrap();
        assert_eq!(text.trim(), r#"language = "tr""#);
        let back: Wrap = toml::from_str(r#"language = "en""#).unwrap();
        assert_eq!(back.language, Language::English);
        assert_eq!(Language::default(), Language::English);
    }
}
