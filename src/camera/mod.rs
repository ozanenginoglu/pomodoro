//! Kamerayla mola koruması: kullanıcı bilgisayarın başında mı?
//!
//! Kamera yalnızca mola korunurken açıktır (`Watch` yaşadığı sürece) ve kendi
//! iş parçacığında çalışır. Kareler yalnızca bellekte işlenir; hiçbir yere
//! kaydedilmez ya da gönderilmez. Saniyede iki kareye bakılır.

mod detector;
mod frame;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as backend;

use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use detector::Detector;

/// Kareler arası süre.
const SAMPLE: Duration = Duration::from_millis(500);
/// Bu kadar süre yüz görülmezse kullanıcı uzakta sayılır…
const AWAY_AFTER: Duration = Duration::from_secs(3);
/// …bu kadar süre üst üste görülürse bilgisayar başında.
const BACK_AFTER: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Ayarlarda saklanan kimlik (Linux'ta aygıt yolu).
    pub id: String,
    pub name: String,
}

/// Görüntü yakalayabilen kameralar.
pub fn devices() -> Vec<Device> {
    backend::devices()
}

/// Kameranın son gördüğü.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sighting {
    /// Gecikmeli karar: kullanıcı bilgisayar başında mı?
    pub present: bool,
    /// Son karedeki yüzün merkezi (kameranın gözünden, 0 ..= 1).
    pub face: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Starting,
    Running(Sighting),
    Failed,
}

/// Kamerayı tutan iş parçacığı. `Watch` bırakılınca iş parçacığı ancak o
/// sıradaki kare gelince (en fazla kare zaman aşımı kadar sonra) aygıtı bırakır;
/// hemen ardından açılan yeni `Watch` aygıtı meşgul bulmasın diye önce bunu bekler.
static DEVICE: Mutex<()> = Mutex::new(());

/// Çalışan kamera. Bırakılınca (drop) kamera kapanır.
pub struct Watch {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
}

impl Watch {
    /// `device`: `Device::id`; `None` ise ilk kamera.
    pub fn start(device: Option<String>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(State::Starting));
        let (thread_stop, thread_state) = (stop.clone(), state.clone());
        let spawned = std::thread::Builder::new().name("camera".into()).spawn(move || {
            let result = run(device.as_deref(), &thread_stop, &thread_state);
            if let Err(e) = result {
                log::warn!("kamera: {e:#}");
                set(&thread_state, State::Failed);
            }
            log::info!("kamera kapandı");
        });
        if spawned.is_err() {
            set(&state, State::Failed);
        }
        Self { stop, state }
    }

    /// Henüz bir karar yoksa ya da kamera açılamadıysa `None`.
    pub fn sighting(&self) -> Option<Sighting> {
        match *self.state.lock().ok()? {
            State::Running(sighting) => Some(sighting),
            _ => None,
        }
    }

    /// Kamera açılamadı ya da görüntü kesildi.
    pub fn failed(&self) -> bool {
        self.state.lock().is_ok_and(|s| *s == State::Failed)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn set(state: &Mutex<State>, value: State) {
    if let Ok(mut state) = state.lock() {
        *state = value;
    }
}

fn run(device: Option<&str>, stop: &AtomicBool, state: &Mutex<State>) -> anyhow::Result<()> {
    // Bekçi en son bırakılır: önce `capture` kapanır.
    let _device = DEVICE.lock().unwrap_or_else(PoisonError::into_inner);
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Kamera açılamıyorsa model hiç hazırlanmasın.
    let mut capture = backend::Capture::open(device)?;
    log::info!("kamera açıldı: {}", capture.describe());
    let detector = Detector::new()?;
    let mut filter = PresenceFilter::default();
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        // Kareler sürekli okunur ki tampondaki eski kareler birikmesin;
        // yalnızca örnekleme anında modele verilir.
        let frame = capture.next_frame()?;
        let now = Instant::now();
        if now < next {
            continue;
        }
        next = now + SAMPLE;
        let Some(input) = frame::to_input(&frame) else {
            log::debug!("kamera: çözülemeyen kare ({:?})", frame.format);
            continue;
        };
        let face = detector.detect(input)?;
        let present = filter.update(now, face.is_some());
        log::trace!("kamera: yüz {face:?}, bilgisayar başında: {present}");
        set(state, State::Running(Sighting { present, face: face.map(|f| f.center) }));
    }
    Ok(())
}

/// Tek karelik hataları ve titremeyi yutan karar. Başta "uzakta" kabul edilir.
#[derive(Default)]
struct PresenceFilter {
    present: bool,
    /// Ham sonucun son değiştiği an.
    since: Option<Instant>,
    last: bool,
}

impl PresenceFilter {
    fn update(&mut self, now: Instant, face: bool) -> bool {
        if self.since.is_none() || face != self.last {
            self.last = face;
            self.since = Some(now);
        }
        let held = now.saturating_duration_since(self.since.unwrap_or(now));
        if face && !self.present && held >= BACK_AFTER {
            self.present = true;
        } else if !face && self.present && held >= AWAY_AFTER {
            self.present = false;
        }
        self.present
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Her yarım saniyede bir sonuç verip son kararı döndürür.
    fn feed(filter: &mut PresenceFilter, now: &mut Instant, face: bool, seconds: f32) -> bool {
        let mut present = filter.present;
        for _ in 0..(seconds / 0.5) as usize {
            *now += SAMPLE;
            present = filter.update(*now, face);
        }
        present
    }

    #[test]
    fn presence_is_debounced() {
        let mut now = Instant::now();
        let mut f = PresenceFilter::default();
        assert!(!feed(&mut f, &mut now, true, 0.5), "tek kare yetmez");
        assert!(feed(&mut f, &mut now, true, 1.0), "bir saniye sonra bilgisayar başında");
        assert!(feed(&mut f, &mut now, false, 2.5), "kısa bir kayıp (başını çevirmek) sayılmaz");
        assert!(feed(&mut f, &mut now, true, 0.5));
        assert!(!feed(&mut f, &mut now, false, 3.5), "3 saniye yüz yoksa uzakta");
    }
}
