//! Kalıcı ayarlar: `~/.config/pomodoro/config.toml` (Windows'ta
//! `%APPDATA%\pomodoro\config\config.toml`). Dosyada eksik alanlar varsayılan
//! değeri alır; elle düzenlenebilir.

use std::{fs, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    i18n::Language,
    meeting::{MeetingDisplay, MeetingSettings},
    pomodoro::PomodoroSettings,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Arayüz dili. Tablolardan önce yazılmalı (TOML kuralı), bu yüzden ilk alan.
    pub language: Language,
    pub pomodoro: PomodoroConfig,
    pub meeting: MeetingConfig,
    pub notifications: NotificationConfig,
    pub appearance: AppearanceConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PomodoroConfig {
    pub work_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    pub rounds_before_long_break: u32,
    pub auto_start_breaks: bool,
    pub auto_start_work: bool,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {
            work_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            rounds_before_long_break: 4,
            auto_start_breaks: true,
            auto_start_work: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    Remaining,
    EndTime,
    #[default]
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingConfig {
    pub display: Display,
    pub warning_minutes: u32,
    pub critical_minutes: u32,
}

impl Default for MeetingConfig {
    fn default() -> Self {
        Self { display: Display::Both, warning_minutes: 5, critical_minutes: 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub sound: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self { enabled: true, sound: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    /// Widget'ın kenar uzunluğu (mantıksal piksel).
    pub fn pixels(self) -> f32 {
        match self {
            Size::Small => 160.0,
            Size::Medium => 200.0,
            Size::Large => 240.0,
        }
    }
}

/// Widget'ın ekrandaki yeri. Wayland'de (layer-shell) ve X11'de açılışta
/// uygulanır. Widget sürüklenince (Windows'ta doğrudan, xmonad'da mod+sürükle)
/// `Custom` olur ve `AppearanceConfig::custom` konumu hatırlanır.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    #[default]
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub size: Size,
    pub position: Position,
    /// Köşeye konumlandırırken ekran kenarından uzaklık (mantıksal piksel).
    pub margin: u32,
    /// Sürüklenerek bırakılan konum (sol üst köşe, mantıksal piksel, tüm
    /// ekranların ortak koordinatlarında). Yalnızca `position = "custom"` iken kullanılır.
    pub custom: Option<[i32; 2]>,
    /// Diğer pencerelerin üstünde dursun. Kapalıyken Wayland'de masaüstü
    /// katmanına (pencerelerin altına) yerleşir.
    pub always_on_top: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            size: Size::default(),
            position: Position::default(),
            margin: 24,
            custom: None,
            always_on_top: true,
        }
    }
}

/// Ayarlar panelindeki sınırlar; dosyadan okunan değerler de bunlara kırpılır.
pub mod limits {
    use std::ops::RangeInclusive;
    pub const WORK: RangeInclusive<u32> = 1..=180;
    pub const BREAK: RangeInclusive<u32> = 1..=60;
    pub const ROUNDS: RangeInclusive<u32> = 1..=12;
    pub const WARNING: RangeInclusive<u32> = 1..=30;
    pub const CRITICAL: RangeInclusive<u32> = 1..=10;
    pub const MARGIN: RangeInclusive<u32> = 0..=400;
}

const fn minutes(m: u32) -> Duration {
    Duration::from_secs(m as u64 * 60)
}

impl Config {
    /// Değerleri geçerli aralıklara kırpar; son uyarı, uyarıdan büyük olamaz.
    pub fn normalized(mut self) -> Self {
        let clamp =
            |v: &mut u32, r: std::ops::RangeInclusive<u32>| *v = (*v).clamp(*r.start(), *r.end());
        let p = &mut self.pomodoro;
        clamp(&mut p.work_minutes, limits::WORK);
        clamp(&mut p.short_break_minutes, limits::BREAK);
        clamp(&mut p.long_break_minutes, limits::BREAK);
        clamp(&mut p.rounds_before_long_break, limits::ROUNDS);
        let m = &mut self.meeting;
        clamp(&mut m.warning_minutes, limits::WARNING);
        clamp(&mut m.critical_minutes, limits::CRITICAL);
        m.critical_minutes = m.critical_minutes.min(m.warning_minutes);
        clamp(&mut self.appearance.margin, limits::MARGIN);
        let a = &mut self.appearance;
        if a.position == Position::Custom && a.custom.is_none() {
            a.position = Position::default();
        }
        self
    }

    pub fn pomodoro_settings(&self) -> PomodoroSettings {
        let p = &self.pomodoro;
        PomodoroSettings {
            work: minutes(p.work_minutes),
            short_break: minutes(p.short_break_minutes),
            long_break: minutes(p.long_break_minutes),
            rounds_before_long_break: p.rounds_before_long_break,
            auto_start_breaks: p.auto_start_breaks,
            auto_start_work: p.auto_start_work,
        }
    }

    pub fn meeting_settings(&self) -> MeetingSettings {
        let m = &self.meeting;
        MeetingSettings {
            warning: minutes(m.warning_minutes),
            critical: minutes(m.critical_minutes),
            display: match m.display {
                Display::Remaining => MeetingDisplay::Remaining,
                Display::EndTime => MeetingDisplay::EndTime,
                Display::Both => MeetingDisplay::Both,
            },
        }
    }

    pub fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "pomodoro")
            .map(|d| d.config_dir().join("config.toml"))
    }

    /// Dosyayı okur. Yoksa varsayılanlar. Bozuksa dosyayı `.broken` olarak
    /// kenara alır (sonraki kayıt üzerine yazmasın) ve varsayılanlarla devam eder.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(config) => config.normalized(),
                Err(e) => {
                    log::warn!("{} okunamadı, varsayılanlar kullanılıyor: {e}", path.display());
                    let _ = fs::rename(&path, path.with_extension("toml.broken"));
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                log::warn!("{} okunamadı: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
            // Önce geçici dosyaya yaz, sonra taşı: yarım kalmış dosya olmasın.
            let tmp = path.with_extension("toml.tmp");
            fs::write(&tmp, text)?;
            fs::rename(&tmp, &path)
        })();
        if let Err(e) = result {
            log::warn!("{} yazılamadı: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_toml() {
        let mut config = Config::default();
        config.pomodoro.work_minutes = 50;
        config.meeting.display = Display::EndTime;
        config.appearance.size = Size::Large;
        config.appearance.position = Position::TopRight;
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("display = \"end_time\""), "{text}");
        assert!(text.contains("position = \"top_right\""), "{text}");
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let config: Config = toml::from_str("[pomodoro]\nwork_minutes = 50\n").unwrap();
        assert_eq!(config.pomodoro.work_minutes, 50);
        assert_eq!(config.pomodoro.short_break_minutes, 5);
        assert_eq!(config.meeting, MeetingConfig::default());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let text = "[pomodoro]\nwork_minutes = 0\nrounds_before_long_break = 99\n\
                    [meeting]\nwarning_minutes = 2\ncritical_minutes = 8\n";
        let config = toml::from_str::<Config>(text).unwrap().normalized();
        assert_eq!(config.pomodoro.work_minutes, 1);
        assert_eq!(config.pomodoro.rounds_before_long_break, 12);
        assert_eq!(config.meeting.critical_minutes, 2, "son uyarı ≤ uyarı");
    }

    #[test]
    fn converts_to_runtime_settings() {
        let config = Config::default();
        assert_eq!(config.pomodoro_settings(), PomodoroSettings::default());
        assert_eq!(config.meeting_settings(), MeetingSettings::default());
    }
}
