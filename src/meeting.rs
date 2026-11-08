//! Toplantı sayacı: süre ("45", "1h30") ya da bitiş saati ("14:30") ile
//! başlar; son dakikalarda uyarır, bitince süre aşımını gösterir.

use std::time::{Duration, Instant};

use chrono::{NaiveDateTime, NaiveTime};

use crate::timer::{Countdown, Remaining};

/// Toplantı en fazla bu kadar sürebilir (yanlış girişlere karşı).
const MAX_MEETING: Duration = Duration::from_secs(12 * 3600);

/// Kullanıcının yazdığı şey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeetingInput {
    /// "45", "45m", "45dk", "1h", "1h30", "1sa30", "90 dk"
    Duration(Duration),
    /// "14:30", "14.30", "9:05". İki nokta/nokta her zaman saat anlamına gelir.
    EndsAt(NaiveTime),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("boş giriş")]
    Empty,
    #[error("anlaşılamadı")]
    Invalid(String),
    #[error("süre sıfır olamaz")]
    Zero,
    #[error("toplantı 12 saatten uzun olamaz")]
    TooLong,
}

pub fn parse(input: &str) -> Result<MeetingInput, ParseError> {
    let s: String = input.trim().to_lowercase().split_whitespace().collect();
    if s.is_empty() {
        return Err(ParseError::Empty);
    }
    let invalid = || ParseError::Invalid(input.trim().to_string());

    if let Some((h, m)) = s.split_once([':', '.']) {
        let (h, m) = (h.parse::<u32>().map_err(|_| invalid())?, m.parse::<u32>().map_err(|_| invalid())?);
        if m >= 60 || s.len() - s.find([':', '.']).unwrap() != 3 {
            return Err(invalid());
        }
        return NaiveTime::from_hms_opt(h, m, 0).map(MeetingInput::EndsAt).ok_or_else(invalid);
    }

    // Saat birimi: "h", "sa", "saat". Dakika birimi: "m", "dk", "min", "dakika" ya da hiçbiri.
    let (hours, rest) = match ["saat", "sa", "h"].iter().find_map(|u| s.split_once(u)) {
        Some((h, rest)) => (h.parse::<u64>().map_err(|_| invalid())?, rest),
        None => (0, s.as_str()),
    };
    let minutes_str = ["dakika", "min", "dk", "m"]
        .iter()
        .find_map(|u| rest.strip_suffix(u))
        .unwrap_or(rest);
    let minutes = match minutes_str {
        "" => 0,
        m => m.parse::<u64>().map_err(|_| invalid())?,
    };
    // "1h75" gibi saatli girişlerde dakika 60'ı geçmemeli; yalın "90" serbest.
    if hours > 0 && minutes >= 60 {
        return Err(invalid());
    }

    let total = Duration::from_secs((hours * 60 + minutes) * 60);
    if total.is_zero() {
        return Err(ParseError::Zero);
    }
    if total > MAX_MEETING {
        return Err(ParseError::TooLong);
    }
    Ok(MeetingInput::Duration(total))
}

/// Girişi, `now` anına göre (süre, bitiş zamanı) çiftine çevirir.
/// - Süre girildiyse toplantı tam o kadar sürer (bitiş saniyeli olabilir).
/// - Saat girildiyse toplantı tam o saatte (:00 saniyede) biter.
/// - Geçmişte kalan bir saat ertesi güne aittir ("23:50"de girilen "00:30").
pub fn resolve(input: MeetingInput, now: NaiveDateTime) -> Result<(Duration, NaiveDateTime), ParseError> {
    let (duration, ends_at) = match input {
        MeetingInput::Duration(d) => (d, now + d),
        MeetingInput::EndsAt(t) => {
            let mut ends_at = now.date().and_time(t);
            if ends_at <= now {
                ends_at += chrono::Duration::days(1);
            }
            ((ends_at - now).to_std().unwrap(), ends_at)
        }
    };
    if duration > MAX_MEETING {
        return Err(ParseError::TooLong);
    }
    Ok((duration, ends_at))
}

/// Toplantı sırasında dairede ne gösterilsin?
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeetingDisplay {
    /// Büyük: kalan süre.
    Remaining,
    /// Büyük: bitiş saati.
    EndTime,
    /// Büyük: kalan süre, altında küçük: bitiş saati.
    #[default]
    Both,
}

/// Uyarı eşikleri ve gösterim.
#[derive(Clone, Debug, PartialEq)]
pub struct MeetingSettings {
    pub warning: Duration,
    pub critical: Duration,
    pub display: MeetingDisplay,
}

impl Default for MeetingSettings {
    fn default() -> Self {
        Self {
            warning: Duration::from_secs(5 * 60),
            critical: Duration::from_secs(60),
            display: MeetingDisplay::default(),
        }
    }
}

/// Halkanın rengini belirleyen aşama.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Normal,
    Warning,
    Critical,
    Overtime,
}

#[derive(Clone, Debug)]
pub struct Meeting {
    timer: Countdown,
    ends_at: NaiveDateTime,
    settings: MeetingSettings,
    /// Bildirimleri bir kez göndermek için en son bildirilen aşama.
    reported: Stage,
}

impl Meeting {
    /// Toplantıyı hemen başlatır.
    pub fn start(
        input: MeetingInput,
        wall_now: NaiveDateTime,
        now: Instant,
        settings: MeetingSettings,
    ) -> Result<Self, ParseError> {
        let (duration, ends_at) = resolve(input, wall_now)?;
        let mut timer = Countdown::new(duration);
        timer.start(now);
        let mut meeting = Self { timer, ends_at, settings, reported: Stage::Normal };
        // Zaten son 5 dakikadaysa, başlarken "uyarı" bildirimi gönderme.
        meeting.reported = meeting.stage(now);
        Ok(meeting)
    }

    pub fn ends_at(&self) -> NaiveDateTime {
        self.ends_at
    }

    pub fn timer(&self) -> &Countdown {
        &self.timer
    }

    /// Eşikleri günceller. Daha önce bildirilmiş aşamalar tekrar bildirilmez.
    pub fn set_settings(&mut self, settings: MeetingSettings) {
        self.settings = settings;
    }

    pub fn remaining(&self, now: Instant) -> Remaining {
        self.timer.remaining(now)
    }

    pub fn stage(&self, now: Instant) -> Stage {
        match self.timer.remaining(now) {
            Remaining::Over(_) => Stage::Overtime,
            Remaining::Left(d) if d.is_zero() => Stage::Overtime,
            Remaining::Left(d) if d <= self.settings.critical => Stage::Critical,
            Remaining::Left(d) if d <= self.settings.warning => Stage::Warning,
            Remaining::Left(_) => Stage::Normal,
        }
    }

    /// Periyodik olarak çağrılır; yeni bir aşamaya geçildiyse onu döndürür.
    /// Birden fazla eşik aynı anda geçildiyse yalnızca en sonuncusu bildirilir.
    pub fn tick(&mut self, now: Instant) -> Option<Stage> {
        let stage = self.stage(now);
        (stage > self.reported).then(|| {
            self.reported = stage;
            stage
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    const MIN: Duration = Duration::from_secs(60);

    fn at(h: u32, m: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 8).unwrap().and_hms_opt(h, m, s).unwrap()
    }
    fn dur(m: u64) -> MeetingInput {
        MeetingInput::Duration(MIN * m as u32)
    }
    fn time(h: u32, m: u32) -> MeetingInput {
        MeetingInput::EndsAt(NaiveTime::from_hms_opt(h, m, 0).unwrap())
    }

    #[test]
    fn parses_durations() {
        for (input, minutes) in [
            ("45", 45),
            (" 45 ", 45),
            ("45m", 45),
            ("45dk", 45),
            ("45 dk", 45),
            ("45min", 45),
            ("90", 90),
            ("1h", 60),
            ("1h30", 90),
            ("1h30m", 90),
            ("1 saat 15 dk", 75),
            ("1sa30", 90),
            ("2H", 120),
        ] {
            assert_eq!(parse(input), Ok(dur(minutes)), "{input:?}");
        }
    }

    #[test]
    fn parses_end_times() {
        assert_eq!(parse("14:30"), Ok(time(14, 30)));
        assert_eq!(parse("14.30"), Ok(time(14, 30)));
        assert_eq!(parse("9:05"), Ok(time(9, 5)));
        assert_eq!(parse("0:00"), Ok(time(0, 0)));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse(""), Err(ParseError::Empty));
        assert_eq!(parse("0"), Err(ParseError::Zero));
        assert_eq!(parse("13h"), Err(ParseError::TooLong));
        for bad in ["abc", "25:00", "14:3", "14:300", "14:60", "1h75", "-5", "1.5h", "h"] {
            assert!(matches!(parse(bad), Err(ParseError::Invalid(_))), "{bad:?} → {:?}", parse(bad));
        }
    }

    #[test]
    fn resolves_duration_as_exact_length() {
        let (d, end) = resolve(dur(45), at(14, 0, 37)).unwrap();
        assert_eq!(d, 45 * MIN);
        assert_eq!(end, at(14, 45, 37));
    }

    #[test]
    fn resolves_end_time_today_or_tomorrow() {
        // Bitiş tam 14:30:00; kalan süre saniyeleriyle hesaplanır.
        let (d, end) = resolve(time(14, 30), at(13, 45, 20)).unwrap();
        assert_eq!((d, end), (44 * MIN + Duration::from_secs(40), at(14, 30, 0)));

        // Geçmişte kalan saat ertesi gün; ama 12 saat sınırını aşarsa hata.
        let (d, _) = resolve(time(0, 30), at(23, 50, 0)).unwrap();
        assert_eq!(d, 40 * MIN);
        assert_eq!(resolve(time(9, 0), at(10, 0, 0)), Err(ParseError::TooLong));
        // Tam şu an girilen saat de ertesi güne düşer → çok uzun.
        assert_eq!(resolve(time(14, 0), at(14, 0, 0)), Err(ParseError::TooLong));
    }

    #[test]
    fn stages_follow_thresholds() {
        let t0 = Instant::now();
        let m = Meeting::start(dur(30), at(14, 0, 0), t0, MeetingSettings::default()).unwrap();
        assert_eq!(m.stage(t0), Stage::Normal);
        assert_eq!(m.stage(t0 + 25 * MIN), Stage::Warning);
        assert_eq!(m.stage(t0 + 29 * MIN), Stage::Critical);
        assert_eq!(m.stage(t0 + 30 * MIN), Stage::Overtime);
        assert_eq!(m.remaining(t0 + 33 * MIN), Remaining::Over(3 * MIN));
        assert_eq!(m.ends_at(), at(14, 30, 0));
    }

    #[test]
    fn tick_reports_each_stage_once() {
        let t0 = Instant::now();
        let mut m = Meeting::start(dur(30), at(14, 0, 0), t0, MeetingSettings::default()).unwrap();
        assert_eq!(m.tick(t0 + 10 * MIN), None);
        assert_eq!(m.tick(t0 + 25 * MIN), Some(Stage::Warning));
        assert_eq!(m.tick(t0 + 26 * MIN), None);
        assert_eq!(m.tick(t0 + 29 * MIN), Some(Stage::Critical));
        assert_eq!(m.tick(t0 + 31 * MIN), Some(Stage::Overtime));
        assert_eq!(m.tick(t0 + 40 * MIN), None);
    }

    #[test]
    fn skipped_thresholds_report_only_latest() {
        let t0 = Instant::now();
        let mut m = Meeting::start(dur(30), at(14, 0, 0), t0, MeetingSettings::default()).unwrap();
        assert_eq!(m.tick(t0 + 35 * MIN), Some(Stage::Overtime));
    }

    #[test]
    fn short_meeting_does_not_warn_immediately() {
        let t0 = Instant::now();
        let mut m = Meeting::start(dur(3), at(14, 0, 0), t0, MeetingSettings::default()).unwrap();
        assert_eq!(m.stage(t0), Stage::Warning);
        assert_eq!(m.tick(t0), None, "başlarken uyarı bildirimi gelmemeli");
        assert_eq!(m.tick(t0 + 2 * MIN + Duration::from_secs(1)), Some(Stage::Critical));
    }
}
