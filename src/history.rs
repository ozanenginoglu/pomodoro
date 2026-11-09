//! Tur geçmişi: süresi dolan ya da atlanan her tur, bir JSON satırı olarak
//! `~/.local/share/pomodoro/history.jsonl` dosyasına eklenir (Windows'ta
//! `%APPDATA%\pomodoro\data\history.jsonl`). Dosya yalnızca büyür; elle
//! silinebilir ya da kırpılabilir.
//!
//! ```json
//! {"end":"2026-11-09T14:30:00+03:00","phase":"work","outcome":"done","seconds":1500}
//! ```

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use chrono::{DateTime, Local, NaiveDate, SecondsFormat};
use serde::{Deserialize, Serialize};

use crate::pomodoro::Phase;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Süresi doldu.
    Done,
    /// Süresi dolmadan atlandı.
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Turun bittiği ya da atlandığı an; yerel saat, RFC 3339.
    pub end: String,
    pub phase: Phase,
    pub outcome: Outcome,
    /// Turun işlediği süre (saniye). Atlanan turda o ana kadar geçen süre.
    pub seconds: u64,
}

impl Entry {
    pub fn new(end: DateTime<Local>, phase: Phase, outcome: Outcome, counted: Duration) -> Self {
        Self {
            end: end.to_rfc3339_opts(SecondsFormat::Secs, false),
            phase,
            outcome,
            seconds: counted.as_secs(),
        }
    }

    /// Turun bittiği gün, şimdiki saat dilimine göre.
    fn local_date(&self) -> Option<NaiveDate> {
        DateTime::parse_from_rfc3339(&self.end).ok().map(|t| t.with_timezone(&Local).date_naive())
    }
}

pub fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "pomodoro").map(|d| d.data_dir().join("history.jsonl"))
}

/// Kaydı dosyaya ekler; diske yazmak arayüzü bekletmesin diye ayrı iş parçacığında.
pub fn record(entry: Entry) {
    let Some(path) = path() else {
        return;
    };
    thread::spawn(move || {
        if let Err(e) = append(&path, &entry) {
            log::warn!("{} yazılamadı: {e}", path.display());
        }
    });
}

/// Verilen günün çalışma turları, sırayla (domates bahçesi için).
pub fn work_rounds_on(date: NaiveDate) -> Vec<Outcome> {
    let Some(path) = path() else {
        return Vec::new();
    };
    match fs::read_to_string(&path) {
        Ok(text) => work_rounds_in(&text, date),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            log::warn!("{} okunamadı: {e}", path.display());
            Vec::new()
        }
    }
}

/// Bozuk ya da tanınmayan satırlar atlanır. Tarih metni geçmeyen satırlar
/// ayrıştırılmaz; dosya yıllar içinde büyüse de hızlı kalsın.
fn work_rounds_in(text: &str, date: NaiveDate) -> Vec<Outcome> {
    let day = date.format("%Y-%m-%d").to_string();
    text.lines()
        .filter(|line| line.contains(&day))
        .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
        .filter(|e| e.phase == Phase::Work && e.local_date() == Some(date))
        .map(|e| e.outcome)
        .collect()
}

fn append(path: &Path, entry: &Entry) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    // Tek bir `write` çağrısı: aynı anda eklenen iki satır birbirine karışmasın.
    OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn writes_one_json_line_per_entry() {
        let end = Local.with_ymd_and_hms(2026, 11, 9, 14, 30, 0).unwrap();
        let entry = Entry::new(end, Phase::Work, Outcome::Done, Duration::from_secs(1500));
        let path = std::env::temp_dir()
            .join(format!("pomodoro-history-test-{}", std::process::id()))
            .join("history.jsonl");
        let _ = fs::remove_file(&path);
        append(&path, &entry).unwrap();
        let skipped = Entry::new(end, Phase::ShortBreak, Outcome::Skipped, Duration::from_secs(61));
        append(&path, &skipped).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());

        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(r#"{"end":"2026-11-09T14:30:00"#), "{}", lines[0]);
        assert!(lines[0].ends_with(r#""phase":"work","outcome":"done","seconds":1500}"#));
        assert!(lines[1].contains(r#""phase":"short_break","outcome":"skipped","seconds":61"#));
        assert_eq!(serde_json::from_str::<Entry>(lines[0]).unwrap(), entry);
        assert!(DateTime::parse_from_rfc3339(&entry.end).is_ok());
    }

    #[test]
    fn reads_one_days_work_rounds_in_order() {
        let at = |d, h| Local.with_ymd_and_hms(2026, 11, d, h, 0, 0).unwrap();
        let line = |e: Entry| serde_json::to_string(&e).unwrap();
        let min = Duration::from_secs(60);
        let text = [
            line(Entry::new(at(8, 23), Phase::Work, Outcome::Done, min)),
            line(Entry::new(at(9, 9), Phase::Work, Outcome::Done, min)),
            line(Entry::new(at(9, 10), Phase::ShortBreak, Outcome::Done, min)),
            "{bozuk satır 2026-11-09".into(),
            line(Entry::new(at(9, 11), Phase::Work, Outcome::Skipped, min)),
            line(Entry::new(at(9, 12), Phase::Work, Outcome::Done, min)),
            line(Entry::new(at(10, 0), Phase::Work, Outcome::Done, min)),
        ]
        .join("\n");
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert_eq!(
            work_rounds_in(&text, day),
            [Outcome::Done, Outcome::Skipped, Outcome::Done],
            "yalnızca o günün çalışma turları; bozuk satır atlanır"
        );
        assert!(work_rounds_in("", day).is_empty());
    }
}
