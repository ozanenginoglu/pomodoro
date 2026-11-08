//! `pomodoro status` çıktısı: xmobar, waybar (JSON) ya da düz metin.

use serde_json::json;

/// Widget'ın paylaştığı, biçimden bağımsız durum.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum StatusInfo {
    /// Widget açık ama hiçbir sayaç çalışmıyor.
    #[default]
    Idle,
    Pomodoro {
        remaining: String,
        on_break: bool,
        paused: bool,
        color: String,
    },
    Meeting {
        remaining: String,
        ends_at: String,
        color: String,
        stage: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Xmobar,
    Waybar,
    Plain,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "xmobar" => Format::Xmobar,
            "waybar" => Format::Waybar,
            "plain" => Format::Plain,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Xmobar => "xmobar",
            Format::Waybar => "waybar",
            Format::Plain => "plain",
        }
    }

    /// Platformun varsayılanı: Linux'ta xmobar, diğerlerinde düz metin.
    pub fn platform_default() -> Self {
        if cfg!(target_os = "linux") { Format::Xmobar } else { Format::Plain }
    }
}

const MUTED: &str = "#6272a4";
// Font Awesome (Solid) ikonları: kronometre, kahve, kişiler.
const STOPWATCH: char = '\u{f2f2}';
const MUG: char = '\u{f7b6}';
const USERS: char = '\u{f0c0}';

pub fn format(info: &StatusInfo, format: Format) -> String {
    match format {
        Format::Xmobar => xmobar(info),
        Format::Waybar => waybar(info),
        Format::Plain => plain(info),
    }
}

/// Widget kapalıyken: xmobar'da hiçbir şey görünmez, waybar boş metinli
/// modülü gizler.
pub fn closed(format: Format) -> String {
    match format {
        Format::Waybar => json!({ "text": "", "class": "closed" }).to_string(),
        Format::Xmobar | Format::Plain => String::new(),
    }
}

/// İkonlar xmobar'daki 4. ek yazı tipinden (Font Awesome Solid) gelir.
fn xmobar(info: &StatusInfo) -> String {
    let icon = |c: char| format!("<fn=4>{c}</fn>");
    match info {
        StatusInfo::Idle => format!("<fc={MUTED}>{}</fc>", icon(STOPWATCH)),
        StatusInfo::Pomodoro { remaining, on_break, paused, color } => {
            let color = if *paused { MUTED } else { color };
            let glyph = if *on_break { MUG } else { STOPWATCH };
            format!("<fc={color}>{}  {remaining}</fc>", icon(glyph))
        }
        StatusInfo::Meeting { remaining, ends_at, color, .. } => {
            format!("<fc={color}>{}  {remaining}</fc> <fc={MUTED}>→ {ends_at}</fc>", icon(USERS))
        }
    }
}

/// Waybar `custom` modülü için (`"return-type": "json"`). Renkler CSS
/// sınıflarıyla verilir: idle, work, break, paused, meeting, warning,
/// critical, overtime.
fn waybar(info: &StatusInfo) -> String {
    let (text, tooltip, class) = match info {
        StatusInfo::Idle => (STOPWATCH.to_string(), "Pomodoro".to_string(), "idle"),
        StatusInfo::Pomodoro { remaining, on_break, paused, .. } => {
            let (glyph, name) = if *on_break { (MUG, "Mola") } else { (STOPWATCH, "Çalışma") };
            let class = if *paused {
                "paused"
            } else if *on_break {
                "break"
            } else {
                "work"
            };
            (format!("{glyph}  {remaining}"), format!("{name} · {remaining}"), class)
        }
        StatusInfo::Meeting { remaining, ends_at, stage, .. } => (
            format!("{USERS}  {remaining}"),
            format!("Toplantı · {remaining} · bitiş {ends_at}"),
            *stage,
        ),
    };
    json!({ "text": text, "tooltip": tooltip, "class": class, "alt": class }).to_string()
}

fn plain(info: &StatusInfo) -> String {
    match info {
        StatusInfo::Idle => String::new(),
        StatusInfo::Pomodoro { remaining, paused, .. } => {
            format!("{remaining}{}", if *paused { " (duraklatıldı)" } else { "" })
        }
        StatusInfo::Meeting { remaining, ends_at, .. } => format!("{remaining} → {ends_at}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> StatusInfo {
        StatusInfo::Pomodoro {
            remaining: "18:42".into(),
            on_break: false,
            paused: false,
            color: "#ff79c6".into(),
        }
    }

    #[test]
    fn xmobar_uses_color_and_icon_font() {
        assert_eq!(format(&work(), Format::Xmobar), "<fc=#ff79c6><fn=4>\u{f2f2}</fn>  18:42</fc>");
    }

    #[test]
    fn waybar_is_valid_json_with_class() {
        let out = format(&work(), Format::Waybar);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["class"], "work");
        assert_eq!(v["tooltip"], "Çalışma · 18:42");
        let meeting = StatusInfo::Meeting {
            remaining: "+00:12".into(),
            ends_at: "14:30".into(),
            color: "#ff5555".into(),
            stage: "overtime",
        };
        let v: serde_json::Value = serde_json::from_str(&format(&meeting, Format::Waybar)).unwrap();
        assert_eq!(v["class"], "overtime");
    }

    #[test]
    fn plain_is_empty_when_idle() {
        assert_eq!(format(&StatusInfo::Idle, Format::Plain), "");
        assert_eq!(format(&work(), Format::Plain), "18:42");
    }
}
