//! Uygulamaya gömülü dosyalar (yalnızca kullandığımız Lucide ikonları).

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*]
    };
}

static FILES: &[(&str, &[u8])] = icons![
    "play",
    "pause",
    "rotate-ccw",
    "skip-forward",
    "settings",
    "users",
    "timer",
    "square",
    "x",
    "arrow-up-left",
    "arrow-up-right",
    "arrow-down-left",
    "arrow-down-right",
    "circle-dot",
    "chevron-left",
    "chevron-right"
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES.iter().find(|(p, _)| *p == path).map(|(_, data)| Cow::Borrowed(*data)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| (*p).into()).collect())
    }
}

/// İkon yolları.
pub mod icon {
    pub const PLAY: &str = "icons/play.svg";
    pub const PAUSE: &str = "icons/pause.svg";
    pub const RESET: &str = "icons/rotate-ccw.svg";
    pub const SKIP: &str = "icons/skip-forward.svg";
    pub const STOP: &str = "icons/square.svg";
    pub const POMODORO: &str = "icons/timer.svg";
    pub const MEETING: &str = "icons/users.svg";
    pub const SETTINGS: &str = "icons/settings.svg";
    pub const CLOSE: &str = "icons/x.svg";
    pub const TOP_LEFT: &str = "icons/arrow-up-left.svg";
    pub const TOP_RIGHT: &str = "icons/arrow-up-right.svg";
    pub const BOTTOM_LEFT: &str = "icons/arrow-down-left.svg";
    pub const BOTTOM_RIGHT: &str = "icons/arrow-down-right.svg";
    pub const CENTER: &str = "icons/circle-dot.svg";
    pub const CHEVRON_LEFT: &str = "icons/chevron-left.svg";
    pub const CHEVRON_RIGHT: &str = "icons/chevron-right.svg";
}
