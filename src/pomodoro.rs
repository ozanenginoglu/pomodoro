//! Pomodoro döngüsü: çalışma → kısa mola → … → uzun mola → çalışma.

use std::time::{Duration, Instant};

use crate::timer::Countdown;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Work,
    ShortBreak,
    LongBreak,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PomodoroSettings {
    pub work: Duration,
    pub short_break: Duration,
    pub long_break: Duration,
    /// Uzun moladan önce tamamlanacak çalışma turu sayısı.
    pub rounds_before_long_break: u32,
    /// Çalışma bitince mola kendiliğinden başlasın mı?
    pub auto_start_breaks: bool,
    /// Mola bitince çalışma kendiliğinden başlasın mı?
    pub auto_start_work: bool,
}

impl Default for PomodoroSettings {
    fn default() -> Self {
        Self {
            work: Duration::from_secs(25 * 60),
            short_break: Duration::from_secs(5 * 60),
            long_break: Duration::from_secs(15 * 60),
            rounds_before_long_break: 4,
            auto_start_breaks: true,
            auto_start_work: false,
        }
    }
}

impl PomodoroSettings {
    pub fn duration_of(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Work => self.work,
            Phase::ShortBreak => self.short_break,
            Phase::LongBreak => self.long_break,
        }
    }

    fn rounds(&self) -> u32 {
        self.rounds_before_long_break.max(1)
    }
}

/// Bir evre süresi dolarak bittiğinde üretilir (bildirim/ses için).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseFinished {
    pub finished: Phase,
    pub next: Phase,
}

#[derive(Clone, Debug)]
pub struct Pomodoro {
    settings: PomodoroSettings,
    phase: Phase,
    /// Mevcut döngüde tamamlanan çalışma turu sayısı.
    completed: u32,
    timer: Countdown,
}

impl Pomodoro {
    pub fn new(settings: PomodoroSettings) -> Self {
        let timer = Countdown::new(settings.work);
        Self { settings, phase: Phase::Work, completed: 0, timer }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn timer(&self) -> &Countdown {
        &self.timer
    }

    /// Ayarları değiştirir. Mevcut evre henüz başlamadıysa yeni süre hemen
    /// uygulanır; çalışan bir evrenin süresi değişmez, sonraki evrede geçerli olur.
    pub fn set_settings(&mut self, settings: PomodoroSettings) {
        if self.timer.state() == crate::timer::TimerState::Idle {
            self.timer.set_duration(settings.duration_of(self.phase));
        }
        self.completed = self.completed.min(settings.rounds() - 1);
        self.settings = settings;
    }

    /// Tur göstergesi için (dolu, toplam).
    pub fn rounds(&self) -> (u32, u32) {
        let total = self.settings.rounds();
        let filled = match self.phase {
            Phase::LongBreak => total,
            _ => self.completed,
        };
        (filled, total)
    }

    pub fn toggle(&mut self, now: Instant) {
        self.timer.toggle(now);
    }

    /// Mevcut evreyi baştan alır (durmuş halde).
    pub fn reset(&mut self) {
        self.timer.reset();
    }

    /// Her şeyi ilk çalışma turuna döndürür.
    pub fn reset_all(&mut self) {
        *self = Self::new(self.settings.clone());
    }

    /// Mevcut evreyi atlar. Atlanan çalışma turu tamamlanmış sayılmaz.
    pub fn skip(&mut self, now: Instant) {
        let next = self.next_phase(false);
        self.enter(next, false, now);
    }

    /// Periyodik olarak çağrılır; süre dolduysa sonraki evreye geçer.
    pub fn tick(&mut self, now: Instant) -> Option<PhaseFinished> {
        if !self.timer.is_running() || !self.timer.is_finished(now) {
            return None;
        }
        let finished = self.phase;
        let next = self.next_phase(true);
        let auto_start = match next {
            Phase::Work => self.settings.auto_start_work,
            _ => self.settings.auto_start_breaks,
        };
        self.enter(next, auto_start, now);
        Some(PhaseFinished { finished, next })
    }

    fn next_phase(&mut self, count_work: bool) -> Phase {
        match self.phase {
            Phase::Work => {
                if !count_work {
                    return Phase::ShortBreak;
                }
                self.completed += 1;
                if self.completed >= self.settings.rounds() {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak => Phase::Work,
            Phase::LongBreak => {
                self.completed = 0;
                Phase::Work
            }
        }
    }

    fn enter(&mut self, phase: Phase, start: bool, now: Instant) {
        self.phase = phase;
        self.timer = Countdown::new(self.settings.duration_of(phase));
        if start {
            self.timer.start(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::{Remaining, TimerState};

    const MIN: Duration = Duration::from_secs(60);

    fn settings() -> PomodoroSettings {
        PomodoroSettings {
            work: 25 * MIN,
            short_break: 5 * MIN,
            long_break: 15 * MIN,
            rounds_before_long_break: 3,
            auto_start_breaks: true,
            auto_start_work: true,
        }
    }

    /// Mevcut evreyi başlatıp sonuna kadar çalıştırır.
    fn finish(p: &mut Pomodoro, now: &mut Instant) -> PhaseFinished {
        if !p.timer().is_running() {
            p.toggle(*now);
        }
        *now += p.timer().duration();
        p.tick(*now).expect("evre bitmeliydi")
    }

    #[test]
    fn starts_idle_in_work() {
        let p = Pomodoro::new(settings());
        assert_eq!(p.phase(), Phase::Work);
        assert_eq!(p.timer().state(), TimerState::Idle);
        assert_eq!(p.rounds(), (0, 3));
    }

    #[test]
    fn no_event_before_time_is_up() {
        let mut now = Instant::now();
        let mut p = Pomodoro::new(settings());
        p.toggle(now);
        now += 24 * MIN;
        assert_eq!(p.tick(now), None);
        assert_eq!(p.phase(), Phase::Work);
    }

    #[test]
    fn full_cycle_reaches_long_break_and_wraps() {
        let mut now = Instant::now();
        let mut p = Pomodoro::new(settings());
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push(finish(&mut p, &mut now).next);
        }
        use Phase::*;
        assert_eq!(seen, [ShortBreak, Work, ShortBreak, Work, LongBreak, Work]);
        assert_eq!(p.rounds(), (0, 3));
    }

    #[test]
    fn round_indicator_follows_progress() {
        let mut now = Instant::now();
        let mut p = Pomodoro::new(settings());
        finish(&mut p, &mut now); // çalışma 1 → kısa mola
        assert_eq!(p.rounds(), (1, 3));
        finish(&mut p, &mut now); // → çalışma 2
        finish(&mut p, &mut now); // → kısa mola
        finish(&mut p, &mut now); // → çalışma 3
        assert_eq!(p.rounds(), (2, 3));
        finish(&mut p, &mut now); // → uzun mola
        assert_eq!(p.phase(), Phase::LongBreak);
        assert_eq!(p.rounds(), (3, 3));
    }

    #[test]
    fn auto_start_settings_are_respected() {
        let mut now = Instant::now();
        let mut p = Pomodoro::new(PomodoroSettings {
            auto_start_breaks: true,
            auto_start_work: false,
            ..settings()
        });
        finish(&mut p, &mut now);
        assert_eq!(p.phase(), Phase::ShortBreak);
        assert!(p.timer().is_running(), "mola kendiliğinden başlamalı");
        finish(&mut p, &mut now);
        assert_eq!(p.phase(), Phase::Work);
        assert_eq!(p.timer().state(), TimerState::Idle, "çalışma beklemeli");
    }

    #[test]
    fn skipping_work_does_not_count_as_completed() {
        let now = Instant::now();
        let mut p = Pomodoro::new(settings());
        p.toggle(now);
        p.skip(now + MIN);
        assert_eq!(p.phase(), Phase::ShortBreak);
        assert_eq!(p.rounds(), (0, 3));
        assert_eq!(p.timer().state(), TimerState::Idle);
        assert_eq!(p.timer().remaining(now), Remaining::Left(5 * MIN));
    }

    #[test]
    fn late_tick_still_advances_once() {
        let mut now = Instant::now();
        let mut p = Pomodoro::new(settings());
        p.toggle(now);
        now += 2 * 60 * MIN; // uyku/askıdan dönüş gibi
        assert!(p.tick(now).is_some());
        assert_eq!(p.phase(), Phase::ShortBreak);
        // Yeni evre "şimdi"den başlar; hemen ikinci bir geçiş olmaz.
        assert_eq!(p.tick(now), None);
    }

    #[test]
    fn settings_change_applies_to_idle_phase_only() {
        let now = Instant::now();
        let mut p = Pomodoro::new(settings());
        p.set_settings(PomodoroSettings { work: 50 * MIN, ..settings() });
        assert_eq!(p.timer().duration(), 50 * MIN);

        p.toggle(now);
        p.set_settings(PomodoroSettings { work: 30 * MIN, ..settings() });
        assert_eq!(p.timer().duration(), 50 * MIN, "çalışan evre değişmemeli");
    }

    #[test]
    fn reset_all_returns_to_first_round() {
        let mut now = Instant::now();
        let mut p = Pomodoro::new(settings());
        finish(&mut p, &mut now);
        p.reset_all();
        assert_eq!(p.phase(), Phase::Work);
        assert_eq!(p.rounds(), (0, 3));
    }
}
