//! Olay komutları (`[hooks]`): Pomodoro ve toplantı olaylarında kullanıcının
//! yazdığı kabuk komutları çalıştırılır. Bildirimleri susturmak, telefona
//! haber göndermek (ntfy, KDE Connect), klavye ışığını boyamak gibi işler
//! uygulamaya ayrıca eklenmeden buradan yapılır.
//!
//! Komutlar tek bir iş parçacığında, geldikleri sırayla çalışır: "bildirimleri
//! sustur" ile "bildirimleri aç" yer değiştirmesin. Takılan bir komut
//! [`TIMEOUT`] sonra sonlandırılır.

use std::{
    process::{Command, Stdio},
    sync::{OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::pomodoro::Phase;

/// Bir komutun en fazla çalışma süresi; uzun işler `&` ile arka plana alınmalı.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// Uygulama kapanırken sıradaki komutların bitmesi için beklenen en uzun süre.
pub const QUIT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Çalışma turu işlemeye başladı (başlatıldı, sürdürüldü ya da kendiliğinden).
    WorkStart,
    /// Çalışma turu işlemeyi bıraktı (bitti, duraklatıldı, sıfırlandı, atlandı).
    WorkStop,
    BreakStart,
    BreakStop,
    /// Çalışma turu süresini doldurdu; masaüstü bildirimiyle aynı an.
    WorkDone,
    BreakDone,
    MeetingStart,
    MeetingWarning,
    MeetingCritical,
    MeetingOvertime,
    /// Toplantı kullanıcı tarafından bitirildi.
    MeetingEnd,
}

impl Event {
    /// Ayar dosyasındaki anahtar; komuta `POMODORO_EVENT` olarak da geçer.
    pub fn key(self) -> &'static str {
        match self {
            Event::WorkStart => "work_start",
            Event::WorkStop => "work_stop",
            Event::BreakStart => "break_start",
            Event::BreakStop => "break_stop",
            Event::WorkDone => "work_done",
            Event::BreakDone => "break_done",
            Event::MeetingStart => "meeting_start",
            Event::MeetingWarning => "meeting_warning",
            Event::MeetingCritical => "meeting_critical",
            Event::MeetingOvertime => "meeting_overtime",
            Event::MeetingEnd => "meeting_end",
        }
    }

    pub fn start(phase: Phase) -> Self {
        if phase == Phase::Work { Event::WorkStart } else { Event::BreakStart }
    }

    pub fn stop(phase: Phase) -> Self {
        if phase == Phase::Work { Event::WorkStop } else { Event::BreakStop }
    }

    pub fn done(phase: Phase) -> Self {
        if phase == Phase::Work { Event::WorkDone } else { Event::BreakDone }
    }
}

/// `POMODORO_PHASE` değeri.
pub fn phase_key(phase: Phase) -> &'static str {
    match phase {
        Phase::Work => "work",
        Phase::ShortBreak => "short_break",
        Phase::LongBreak => "long_break",
    }
}

/// `[hooks]` tablosu: her olay için isteğe bağlı bir kabuk komutu.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HooksConfig {
    pub work_start: Option<String>,
    pub work_stop: Option<String>,
    pub break_start: Option<String>,
    pub break_stop: Option<String>,
    pub work_done: Option<String>,
    pub break_done: Option<String>,
    pub meeting_start: Option<String>,
    pub meeting_warning: Option<String>,
    pub meeting_critical: Option<String>,
    pub meeting_overtime: Option<String>,
    pub meeting_end: Option<String>,
}

impl HooksConfig {
    /// Olayın komutu; boş metin "komut yok" sayılır.
    pub fn command(&self, event: Event) -> Option<&str> {
        let command = match event {
            Event::WorkStart => &self.work_start,
            Event::WorkStop => &self.work_stop,
            Event::BreakStart => &self.break_start,
            Event::BreakStop => &self.break_stop,
            Event::WorkDone => &self.work_done,
            Event::BreakDone => &self.break_done,
            Event::MeetingStart => &self.meeting_start,
            Event::MeetingWarning => &self.meeting_warning,
            Event::MeetingCritical => &self.meeting_critical,
            Event::MeetingOvertime => &self.meeting_overtime,
            Event::MeetingEnd => &self.meeting_end,
        };
        command.as_deref().filter(|c| !c.trim().is_empty())
    }
}

/// Komuta geçen ortam değişkenleri (`POMODORO_` önekiyle).
pub type Env = Vec<(&'static str, String)>;

/// Hangi Pomodoro evresi işliyor? Değiştikçe başla/bitir olaylarını üretir.
/// Mola koruması yüzünden tutulan mola da işliyor sayılır.
#[derive(Debug, Default)]
pub struct Activity {
    running: Option<Phase>,
}

impl Activity {
    /// Yeni durumu kaydeder; (biten, başlayan) evreleri döndürür.
    pub fn update(&mut self, running: Option<Phase>) -> (Option<Phase>, Option<Phase>) {
        if self.running == running {
            return (None, None);
        }
        (std::mem::replace(&mut self.running, running), running)
    }
}

struct Job {
    event: Event,
    command: String,
    env: Env,
}

enum Message {
    Run(Job),
    /// Öncesindeki komutlar bitince haber verir.
    Barrier(mpsc::Sender<()>),
}

fn queue() -> &'static mpsc::Sender<Message> {
    static QUEUE: OnceLock<mpsc::Sender<Message>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Message>();
        thread::spawn(move || {
            for message in rx {
                match message {
                    Message::Run(job) => execute(&job, TIMEOUT),
                    Message::Barrier(done) => drop(done.send(())),
                }
            }
        });
        tx
    })
}

/// Komutu sıraya ekler; hemen döner.
pub fn fire(config: &HooksConfig, event: Event, env: Env) {
    if let Some(command) = config.command(event) {
        let _ = queue().send(Message::Run(Job { event, command: command.to_owned(), env }));
    }
}

/// Sıradaki komutların bitmesini en fazla `timeout` bekler. Uygulama
/// kapanırken çağrılır; yoksa süreçle birlikte yarıda kalırlar.
pub fn flush(timeout: Duration) {
    let (tx, rx) = mpsc::channel();
    if queue().send(Message::Barrier(tx)).is_ok() && rx.recv_timeout(timeout).is_err() {
        log::warn!("olay komutları {} sn içinde bitmedi", timeout.as_secs());
    }
}

/// Komutu kabukta çalıştırır. Başarısız olursa yalnızca günlüğe yazar.
fn execute(job: &Job, timeout: Duration) {
    log::info!("olay komutu ({}): {}", job.event.key(), job.command);
    let mut command = shell(&job.command);
    command.env("POMODORO_EVENT", job.event.key()).envs(job.env.iter().cloned());
    command.stdin(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            log::warn!("{} komutu çalıştırılamadı: {e}", job.event.key());
            return;
        }
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return,
            Ok(Some(status)) => {
                log::warn!("{} komutu başarısız: {status}", job.event.key());
                return;
            }
            Ok(None) if started.elapsed() >= timeout => {
                log::warn!(
                    "{} komutu {} sn içinde bitmedi, sonlandırıldı",
                    job.event.key(),
                    timeout.as_secs()
                );
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                log::warn!("{} komutu beklenemedi: {e}", job.event.key());
                return;
            }
        }
    }
}

#[cfg(not(windows))]
fn shell(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
}

/// Windows'ta `cmd /S /C "…"`: `/S` dıştaki tırnakları atar, içerik olduğu gibi
/// kalır. Konsol penceresi açılmasın diye `CREATE_NO_WINDOW`.
#[cfg(windows)]
fn shell(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = Command::new("cmd");
    c.raw_arg(format!("/S /C \"{command}\"")).creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_commands_from_toml() {
        let text = "work_start = \"dunstctl set-paused true\"\nbreak_done = \"  \"\n";
        let hooks: HooksConfig = toml::from_str(text).unwrap();
        assert_eq!(hooks.command(Event::WorkStart), Some("dunstctl set-paused true"));
        assert_eq!(hooks.command(Event::WorkStop), None);
        assert_eq!(hooks.command(Event::BreakDone), None, "boş komut yok sayılır");
    }

    #[test]
    fn keys_match_config_fields() {
        let all = [
            Event::WorkStart,
            Event::WorkStop,
            Event::BreakStart,
            Event::BreakStop,
            Event::WorkDone,
            Event::BreakDone,
            Event::MeetingStart,
            Event::MeetingWarning,
            Event::MeetingCritical,
            Event::MeetingOvertime,
            Event::MeetingEnd,
        ];
        for event in all {
            let text = format!("{} = \"x\"", event.key());
            let hooks: HooksConfig = toml::from_str(&text).unwrap();
            assert_eq!(hooks.command(event), Some("x"), "{event:?}");
        }
    }

    #[test]
    fn activity_reports_edges_once() {
        let mut a = Activity::default();
        assert_eq!(a.update(None), (None, None));
        assert_eq!(a.update(Some(Phase::Work)), (None, Some(Phase::Work)));
        assert_eq!(a.update(Some(Phase::Work)), (None, None));
        assert_eq!(
            a.update(Some(Phase::ShortBreak)),
            (Some(Phase::Work), Some(Phase::ShortBreak)),
            "kendiliğinden başlayan mola"
        );
        assert_eq!(a.update(None), (Some(Phase::ShortBreak), None));
    }

    #[test]
    fn events_follow_phase() {
        assert_eq!(Event::start(Phase::Work), Event::WorkStart);
        assert_eq!(Event::stop(Phase::LongBreak), Event::BreakStop);
        assert_eq!(Event::done(Phase::ShortBreak), Event::BreakDone);
    }

    #[cfg(unix)]
    #[test]
    fn runs_command_with_environment() {
        let path = std::env::temp_dir().join(format!("pomodoro-hook-test-{}", std::process::id()));
        let hooks = HooksConfig {
            work_done: Some(format!(
                "printf '%s|%s|%s;' \"$POMODORO_EVENT\" \"$POMODORO_PHASE\" \"$POMODORO_MESSAGE\" >> '{}'",
                path.display()
            )),
            work_stop: Some(format!("sleep 0.2; printf 'stop;' >> '{}'", path.display())),
            ..HooksConfig::default()
        };
        let _ = std::fs::remove_file(&path);
        fire(&hooks, Event::WorkStop, Env::new());
        let env = vec![("POMODORO_PHASE", "work".into()), ("POMODORO_MESSAGE", "a b".into())];
        fire(&hooks, Event::WorkDone, env);
        flush(Duration::from_secs(5));
        let written = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(written, "stop;work_done|work|a b;", "sırayla ve ortamla çalışır");
    }

    #[cfg(unix)]
    #[test]
    fn hanging_command_is_killed() {
        let started = Instant::now();
        let job = Job { event: Event::WorkStart, command: "sleep 10".into(), env: Env::new() };
        execute(&job, Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
