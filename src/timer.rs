//! UI'dan bağımsız geri sayım. Zaman "sayılmaz", başlangıç anından
//! hesaplanır; böylece geciken bir tick süreyi kaydırmaz. Test edilebilirlik
//! için "şimdi" her zaman parametre olarak verilir.

use std::time::{Duration, Instant};

/// Geri sayımın durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerState {
    /// Hiç başlatılmadı ya da sıfırlandı.
    Idle,
    Running,
    Paused,
}

/// Kalan süre ya da süre aşımı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remaining {
    Left(Duration),
    Over(Duration),
}

#[derive(Clone, Copy, Debug)]
enum State {
    Idle,
    /// `started_at` anından beri çalışıyor; öncesinde `banked` kadar geçmişti.
    Running { started_at: Instant, banked: Duration },
    Paused { elapsed: Duration },
}

#[derive(Clone, Debug)]
pub struct Countdown {
    duration: Duration,
    state: State,
}

impl Countdown {
    pub fn new(duration: Duration) -> Self {
        Self { duration, state: State::Idle }
    }

    #[cfg(test)]
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Toplam süreyi değiştirir; geçen süre korunur.
    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = duration;
    }

    pub fn state(&self) -> TimerState {
        match self.state {
            State::Idle => TimerState::Idle,
            State::Running { .. } => TimerState::Running,
            State::Paused { .. } => TimerState::Paused,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }

    /// Başlatır ya da duraklatılmışsa devam ettirir.
    pub fn start(&mut self, now: Instant) {
        let banked = match self.state {
            State::Idle => Duration::ZERO,
            State::Paused { elapsed } => elapsed,
            State::Running { .. } => return,
        };
        self.state = State::Running { started_at: now, banked };
    }

    pub fn pause(&mut self, now: Instant) {
        if let State::Running { .. } = self.state {
            self.state = State::Paused { elapsed: self.elapsed(now) };
        }
    }

    pub fn toggle(&mut self, now: Instant) {
        if self.is_running() {
            self.pause(now);
        } else {
            self.start(now);
        }
    }

    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        match self.state {
            State::Idle => Duration::ZERO,
            State::Running { started_at, banked } => {
                banked + now.saturating_duration_since(started_at)
            }
            State::Paused { elapsed } => elapsed,
        }
    }

    pub fn remaining(&self, now: Instant) -> Remaining {
        let elapsed = self.elapsed(now);
        match self.duration.checked_sub(elapsed) {
            Some(left) => Remaining::Left(left),
            None => Remaining::Over(elapsed - self.duration),
        }
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        self.state() != TimerState::Idle && self.elapsed(now) >= self.duration
    }

    /// Geçen sürenin toplam süreye oranı, 0.0..=1.0.
    pub fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (self.elapsed(now).as_secs_f32() / self.duration.as_secs_f32()).min(1.0)
    }
}

/// Ekranda gösterilecek "MM:SS" ya da "H:MM:SS". Saniye kesirleri yukarı
/// yuvarlanır: başlatınca ilk saniye "25:00" görünür, hemen "24:59" değil;
/// "00:00" da ancak süre gerçekten bittiğinde görünür.
pub fn format_clock(d: Duration) -> String {
    let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn idle_timer_shows_full_duration() {
        let t0 = Instant::now();
        let c = Countdown::new(25 * MIN);
        assert_eq!(c.state(), TimerState::Idle);
        assert_eq!(c.remaining(t0 + 10 * MIN), Remaining::Left(25 * MIN));
        assert!(!c.is_finished(t0 + 30 * MIN));
        assert_eq!(c.progress(t0), 0.0);
    }

    #[test]
    fn running_timer_counts_down_from_start() {
        let t0 = Instant::now();
        let mut c = Countdown::new(25 * MIN);
        c.start(t0);
        assert_eq!(c.remaining(t0 + 10 * MIN), Remaining::Left(15 * MIN));
        assert_eq!(c.progress(t0 + 5 * MIN), 0.2);
    }

    #[test]
    fn pause_freezes_and_resume_continues() {
        let t0 = Instant::now();
        let mut c = Countdown::new(25 * MIN);
        c.start(t0);
        c.pause(t0 + 10 * MIN);
        // Duraklatılmışken zaman geçmez.
        assert_eq!(c.remaining(t0 + 40 * MIN), Remaining::Left(15 * MIN));
        c.start(t0 + 40 * MIN);
        assert_eq!(c.remaining(t0 + 45 * MIN), Remaining::Left(10 * MIN));
    }

    #[test]
    fn toggle_alternates_and_double_start_is_noop() {
        let t0 = Instant::now();
        let mut c = Countdown::new(25 * MIN);
        c.toggle(t0);
        assert!(c.is_running());
        c.start(t0 + 5 * MIN); // zaten çalışıyor, başlangıç anı değişmemeli
        assert_eq!(c.remaining(t0 + 5 * MIN), Remaining::Left(20 * MIN));
        c.toggle(t0 + 5 * MIN);
        assert_eq!(c.state(), TimerState::Paused);
    }

    #[test]
    fn overtime_is_reported() {
        let t0 = Instant::now();
        let mut c = Countdown::new(MIN);
        c.start(t0);
        assert!(c.is_finished(t0 + MIN));
        assert_eq!(c.remaining(t0 + 4 * MIN), Remaining::Over(3 * MIN));
        assert_eq!(c.progress(t0 + 4 * MIN), 1.0);
    }

    #[test]
    fn reset_returns_to_idle() {
        let t0 = Instant::now();
        let mut c = Countdown::new(MIN);
        c.start(t0);
        c.reset();
        assert_eq!(c.state(), TimerState::Idle);
        assert_eq!(c.remaining(t0 + MIN), Remaining::Left(MIN));
    }

    #[test]
    fn clock_format_rounds_up_partial_seconds() {
        assert_eq!(format_clock(25 * MIN), "25:00");
        assert_eq!(format_clock(25 * MIN - Duration::from_millis(400)), "25:00");
        assert_eq!(format_clock(25 * MIN - Duration::from_millis(1400)), "24:59");
        assert_eq!(format_clock(Duration::from_secs(5)), "00:05");
        assert_eq!(format_clock(Duration::ZERO), "00:00");
        assert_eq!(format_clock(90 * MIN), "1:30:00");
    }
}
