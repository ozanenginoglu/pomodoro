//! Daire widget'ı: Pomodoro ve toplantı modları, halka, hover'da açılan kontroller.

use std::time::{Duration, Instant};

use gpui::{
    App, Context, Div, Entity, FocusHandle, Hsla, InteractiveElement, IntoElement, KeyBinding,
    KeyDownEvent, Keystroke, MouseButton, MouseMoveEvent, ParentElement, Pixels, Point, Render,
    SharedString, Stateful, Styled, Subscription, Task, Window, WindowHandle, actions, div,
    prelude::*, px, size, svg,
};

use super::{
    ring::{RingStyle, SharedRingCache, ring},
    settings::{self, SettingsPanel},
    theme,
};
use crate::{
    alert::{self, Alert, Chime},
    assets::icon,
    config::{AppearanceConfig, Config, NotificationConfig},
    ipc::{self, Command},
    meeting::{self, Meeting, MeetingDisplay, MeetingSettings, Stage},
    platform::{self, Applied},
    pomodoro::{Phase, PhaseFinished, Pomodoro, PomodoroSettings},
    status::StatusInfo,
    timer::{Remaining, TimerState, format_clock},
};

actions!(pomodoro, [ToggleTimer, ResetTimer, SkipPhase, SwitchMode, OpenSettings]);

// Klavye bağlamları: toplantı süresi yazılırken harf ve boşluk kısayolları
// devre dışı kalsın diye her durumun kendi bağlamı var.
const CTX_POMODORO: &str = "Pomodoro";
const CTX_MEETING: &str = "Meeting";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", ToggleTimer, Some(CTX_POMODORO)),
        KeyBinding::new("r", ResetTimer, Some(CTX_POMODORO)),
        KeyBinding::new("s", SkipPhase, Some(CTX_POMODORO)),
        KeyBinding::new("m", SwitchMode, Some(CTX_POMODORO)),
        KeyBinding::new("r", ResetTimer, Some(CTX_MEETING)),
        KeyBinding::new("m", SwitchMode, Some(CTX_MEETING)),
        KeyBinding::new("tab", SwitchMode, Some("Widget")),
        KeyBinding::new("ctrl-,", OpenSettings, Some("Widget")),
    ]);
}

const FADE: Duration = Duration::from_millis(200);
/// Animasyon sırasında yeniden çizim aralığı (~60 fps).
const FRAME: Duration = Duration::from_millis(16);
/// Kontroller açıkken imlecin hâlâ dairede olup olmadığını yoklama aralığı.
const POINTER_POLL: Duration = Duration::from_millis(100);

// Aşağıdaki ölçüler 200 px'lik (M) widget içindir; diğer boyutlarda orantılı ölçeklenir.
const BASE_SIZE: f32 = 200.0;
const RING_WIDTH: f32 = 6.0;
const CLOCK_SIZE: f32 = 44.0;
const CLOCK_SIZE_HOVER: f32 = 36.0;
/// Hover'da saatin üstünde açılan satır (mod değiştirme).
const TOP_HEIGHT: f32 = 30.0;
/// Normalde saatin altındaki bilgi satırı (tur noktaları / bitiş saati).
const INFO_HEIGHT: f32 = 18.0;
/// Hover'da bilgi satırının yerine açılan kontrol satırı.
const CONTROLS_HEIGHT: f32 = 40.0;
/// Toplantı girişine yazılabilecek en uzun metin.
const INPUT_MAX: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Pomodoro,
    Meeting,
}

/// Hover'da kontrollerin yumuşakça belirmesi için basit bir geçiş.
struct Fade {
    visible: bool,
    from: f32,
    started: Instant,
}

impl Fade {
    fn new() -> Self {
        Self { visible: false, from: 0.0, started: Instant::now() - FADE }
    }

    fn value(&self) -> f32 {
        let t = (self.started.elapsed().as_secs_f32() / FADE.as_secs_f32()).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3); // ease-out cubic
        let to = if self.visible { 1.0 } else { 0.0 };
        self.from + (to - self.from) * eased
    }

    fn is_animating(&self) -> bool {
        self.started.elapsed() < FADE
    }

    fn set(&mut self, visible: bool) {
        if visible != self.visible {
            self.from = self.value();
            self.visible = visible;
            self.started = Instant::now();
        }
    }
}

pub struct Widget {
    config: Entity<Config>,
    settings_window: Option<WindowHandle<SettingsPanel>>,
    /// Boyut ayarına göre ölçek (M = 1.0).
    scale: f32,
    mode: Mode,
    pomodoro: Pomodoro,
    meeting: Option<Meeting>,
    meeting_settings: MeetingSettings,
    notifications: NotificationConfig,
    /// Uygulanmış görünüm ayarları (konum değişikliğini fark etmek için).
    appearance: AppearanceConfig,
    /// Wayland'de konum ancak yeniden açılışta değişir; sayaçlar dururken yapılır.
    pending_restart: bool,
    /// Toplantı süresi/saati girişi.
    input: String,
    input_error: Option<SharedString>,
    focus: FocusHandle,
    controls: Fade,
    /// Geçiş sürerken çalışan yeniden çizim döngüsü.
    animation: Option<Task<()>>,
    /// Kontroller açıkken imleci yoklayan döngü.
    pointer_watch: Option<Task<()>>,
    /// Bir sayaç çalışırken her saniye sınırında ekranı güncelleyen döngü.
    ticker: Option<Task<()>>,
    /// Halka görüntüsü; yalnızca değiştiğinde yeniden çizilir.
    ring: SharedRingCache,
    /// `pomodoro status` için paylaşılan durum satırı.
    status: ipc::Status,
    _config_subscription: Subscription,
    _commands: Task<()>,
}

/// Nokta, pencereye çizilen dairenin içinde mi?
fn in_circle(pos: Point<Pixels>, window: &Window) -> bool {
    let size = window.viewport_size();
    let r = f32::from(size.width.min(size.height)) / 2.0;
    let dx = f32::from(pos.x) - f32::from(size.width) / 2.0;
    let dy = f32::from(pos.y) - f32::from(size.height) / 2.0;
    dx * dx + dy * dy <= r * r
}

impl Widget {
    pub fn new(
        config: Entity<Config>,
        commands: async_channel::Receiver<Command>,
        status: ipc::Status,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let c = config.read(cx).clone();
        let subscription = cx.observe_in(&config, window, |this, config, window, cx| {
            let config = config.read(cx).clone();
            this.apply_config(&config, window, cx);
        });
        // `pomodoro toggle` vb. komut satırı komutları.
        let command_task = cx.spawn(async move |this, cx| {
            while let Ok(command) = commands.recv().await {
                if this.update(cx, |this, cx| this.run_command(command, cx)).is_err() {
                    break;
                }
            }
        });
        let widget = Self {
            config,
            settings_window: None,
            scale: c.appearance.size.pixels() / BASE_SIZE,
            mode: Mode::Pomodoro,
            pomodoro: Pomodoro::new(pomodoro_settings(&c)),
            meeting: None,
            meeting_settings: c.meeting_settings(),
            notifications: c.notifications.clone(),
            appearance: c.appearance.clone(),
            pending_restart: false,
            input: String::new(),
            input_error: None,
            focus,
            controls: Fade::new(),
            animation: None,
            pointer_watch: None,
            ticker: None,
            ring: SharedRingCache::default(),
            status,
            _config_subscription: subscription,
            _commands: command_task,
        };
        widget.publish_status();
        widget
    }

    fn run_command(&mut self, command: Command, cx: &mut Context<Self>) {
        log::info!("komut: {command:?}");
        match command {
            Command::Toggle => self.toggle(cx),
            Command::Reset => {
                self.pomodoro.reset();
                self.changed(cx);
            }
            Command::Skip => self.skip(cx),
            Command::Mode => self.switch_mode(cx),
            Command::Settings => self.open_settings(cx),
        }
    }

    /// Durum satırını günceller (xmobar `pomodoro status` ile okur).
    fn publish_status(&self) {
        let now = Instant::now();
        let left = |r: Remaining| match r {
            Remaining::Left(d) => format_clock(d),
            Remaining::Over(d) => format!("+{}", format_clock(d)),
        };
        let info = if let Some(meeting) = &self.meeting {
            let stage = meeting.stage(now);
            StatusInfo::Meeting {
                remaining: left(meeting.remaining(now)),
                ends_at: meeting.ends_at().format("%H:%M").to_string(),
                color: theme::hex(theme::stage(stage)),
                stage: match stage {
                    Stage::Normal => "meeting",
                    Stage::Warning => "warning",
                    Stage::Critical => "critical",
                    Stage::Overtime => "overtime",
                },
            }
        } else {
            let timer = self.pomodoro.timer();
            match timer.state() {
                TimerState::Idle => StatusInfo::Idle,
                state => StatusInfo::Pomodoro {
                    remaining: left(timer.remaining(now)),
                    on_break: self.pomodoro.phase() != Phase::Work,
                    paused: state == TimerState::Paused,
                    color: theme::hex(theme::phase(self.pomodoro.phase())),
                },
            }
        };
        if let Ok(mut status) = self.status.lock() {
            *status = info;
        }
    }

    /// Ayarlar panelinde yapılan değişiklikleri anında uygular.
    fn apply_config(&mut self, config: &Config, window: &mut Window, cx: &mut Context<Self>) {
        self.pomodoro.set_settings(pomodoro_settings(config));
        self.meeting_settings = config.meeting_settings();
        self.notifications = config.notifications.clone();
        if let Some(meeting) = &mut self.meeting {
            meeting.set_settings(self.meeting_settings.clone());
        }
        let side = config.appearance.size.pixels();
        let scale = side / BASE_SIZE;
        if scale != self.scale {
            self.scale = scale;
            window.resize(size(px(side), px(side)));
        }
        let appearance = &config.appearance;
        let moved = appearance.position != self.appearance.position
            || appearance.margin != self.appearance.margin
            || appearance.always_on_top != self.appearance.always_on_top;
        // Boyut değişince köşedeki widget'ın kenar boşluğu korunsun diye yeniden konumlanır.
        if (moved || appearance.size != self.appearance.size)
            && platform::apply_appearance(window, cx, appearance) == Applied::NeedsRestart
            && moved
        {
            self.pending_restart = true;
        }
        self.appearance = appearance.clone();
        self.changed(cx);
    }

    /// Hiçbir sayaç çalışmıyor ve yarıda kalmış bir evre yok.
    fn is_idle(&self) -> bool {
        self.meeting.is_none() && self.pomodoro.timer().state() == TimerState::Idle
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_window = settings::open(self.settings_window, self.config.clone(), cx);
    }

    // ---- Komutlar --------------------------------------------------------

    fn switch_mode(&mut self, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            Mode::Pomodoro => Mode::Meeting,
            Mode::Meeting => Mode::Pomodoro,
        };
        self.changed(cx);
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        self.pomodoro.toggle(Instant::now());
        self.changed(cx);
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        match self.mode {
            // Durmuş ve baştaki bir evrede "sıfırla", döngüyü tamamen başa alır.
            Mode::Pomodoro if self.pomodoro.timer().state() == TimerState::Idle => {
                self.pomodoro.reset_all()
            }
            Mode::Pomodoro => self.pomodoro.reset(),
            Mode::Meeting => self.end_meeting(),
        }
        self.changed(cx);
    }

    fn skip(&mut self, cx: &mut Context<Self>) {
        self.pomodoro.skip(Instant::now());
        self.changed(cx);
    }

    fn start_meeting(&mut self, cx: &mut Context<Self>) {
        let wall_now = chrono::Local::now().naive_local();
        let result = meeting::parse(&self.input).and_then(|input| {
            Meeting::start(input, wall_now, Instant::now(), self.meeting_settings.clone())
        });
        match result {
            Ok(meeting) => {
                self.meeting = Some(meeting);
                self.input.clear();
                self.input_error = None;
            }
            Err(e) => self.input_error = Some(e.to_string().into()),
        }
        self.changed(cx);
    }

    fn end_meeting(&mut self) {
        self.meeting = None;
        self.input.clear();
        self.input_error = None;
    }

    /// Toplantı girişine gelen tuşlar. İşlendiyse `true`.
    fn input_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &keystroke.modifiers;
        if m.control || m.alt || m.platform {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => {
                self.start_meeting(cx);
                return true;
            }
            "backspace" => {
                self.input.pop();
            }
            "escape" => self.input.clear(),
            _ => {
                let Some(text) = keystroke.key_char.as_deref() else {
                    return false;
                };
                let allowed = |c: char| c.is_alphanumeric() || matches!(c, ':' | '.' | ' ');
                if text.is_empty() || !text.chars().all(allowed) {
                    return false;
                }
                if self.input.chars().count() + text.chars().count() <= INPUT_MAX {
                    self.input.push_str(text);
                }
            }
        }
        self.input_error = None;
        cx.notify();
        true
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.pending_restart && self.is_idle() {
            log::info!("yeni konum için yeniden başlatılıyor");
            cx.restart();
            return;
        }
        cx.notify();
        self.publish_status();
        self.ensure_ticker(cx);
    }

    // ---- Zamanlama ---------------------------------------------------------

    /// Bir sonraki görünür değişikliğe kalan süre; hiçbir sayaç çalışmıyorsa `None`.
    fn next_tick(&self, now: Instant) -> Option<Duration> {
        let pomodoro = self
            .pomodoro
            .timer()
            .is_running()
            .then(|| until_next_second(self.pomodoro.timer().remaining(now)));
        let meeting = self.meeting.as_ref().map(|m| until_next_second(m.remaining(now)));
        pomodoro.into_iter().chain(meeting).min()
    }

    /// Bir sayaç çalıştığı sürece, gösterilen saniye değiştiği anda yeniden
    /// çizer. Hiçbiri çalışmıyorken hiç uyanmaz. İki mod arka planda birlikte
    /// çalışabilir: Pomodoro sürerken toplantıya bakılabilir, ya da tersi.
    fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticker.is_some() || self.next_tick(Instant::now()).is_none() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                let delay = this
                    .read_with(cx, |this, _| this.next_tick(Instant::now()))
                    .ok()
                    .flatten()
                    .unwrap_or(Duration::from_secs(1));
                cx.background_executor().timer(delay).await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        let now = Instant::now();
                        if let Some(event) = this.pomodoro.tick(now) {
                            let alert = this.phase_alert(event);
                            alert::fire(alert, &this.notifications);
                        }
                        if let Some(stage) = this.meeting.as_mut().and_then(|m| m.tick(now)) {
                            let alert = this.meeting_alert(stage);
                            alert::fire(alert, &this.notifications);
                        }
                        cx.notify();
                        this.publish_status();
                        let running = this.next_tick(now).is_some();
                        if !running {
                            this.ticker = None;
                        }
                        running
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        }));
    }

    // ---- Bildirim metinleri -------------------------------------------------

    fn phase_alert(&self, event: PhaseFinished) -> Alert {
        let started = self.pomodoro.timer().is_running();
        let minutes = self.pomodoro.timer().remaining(Instant::now());
        let minutes = match minutes {
            Remaining::Left(d) => d.as_secs().div_ceil(60),
            Remaining::Over(_) => 0,
        };
        let (title, next) = match event.next {
            Phase::ShortBreak => ("Çalışma bitti", "Kısa mola"),
            Phase::LongBreak => ("Çalışma bitti", "Uzun mola"),
            Phase::Work => ("Mola bitti", "Çalışma"),
        };
        let body = if started {
            format!("{next} başladı · {minutes} dk")
        } else {
            format!("Sırada: {} · {minutes} dk", next.to_lowercase())
        };
        Alert { title: title.into(), body, chime: Chime::Phase, urgent: false }
    }

    fn meeting_alert(&self, stage: Stage) -> Alert {
        let end = self
            .meeting
            .as_ref()
            .map(|m| m.ends_at().format("%H:%M").to_string())
            .unwrap_or_default();
        let s = &self.meeting_settings;
        let left = |d: Duration| format!("Bitişe {} dakika kaldı · {end}", d.as_secs() / 60);
        match stage {
            Stage::Warning => Alert {
                title: "Toplantı".into(),
                body: left(s.warning),
                chime: Chime::Gentle,
                urgent: false,
            },
            Stage::Critical => Alert {
                title: "Toplantı".into(),
                body: left(s.critical),
                chime: Chime::Phase,
                urgent: false,
            },
            Stage::Overtime | Stage::Normal => Alert {
                title: "Toplantı süresi doldu".into(),
                body: format!("Bitiş saati {end} idi"),
                chime: Chime::Alarm,
                urgent: true,
            },
        }
    }

    // ---- Hover ---------------------------------------------------------

    fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered == self.controls.visible {
            return;
        }
        log::debug!("hover: {hovered}");
        self.controls.set(hovered);
        cx.notify();
        self.ensure_animation(cx);
        if hovered {
            self.watch_pointer(cx);
        }
    }

    /// X11'de pencere odakta değilken ve fare hareket etmezken animasyon
    /// kareleri gelmeyebiliyor. Bu yüzden `request_animation_frame` yerine
    /// geçiş bitene kadar kendi zamanlayıcımızla yeniden çizim istiyoruz.
    fn ensure_animation(&mut self, cx: &mut Context<Self>) {
        if self.animation.is_some() {
            return;
        }
        self.animation = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        let animating = this.controls.is_animating();
                        if !animating {
                            this.animation = None;
                        }
                        animating
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        }));
    }

    /// GPUI'nin "fare çıktı" olayı X11/xmonad altında her zaman gelmiyor.
    /// Kontroller açık kaldığı sürece imleci doğrudan sorgulayıp daireden
    /// çıkınca kapatıyoruz. Kontroller kapanınca döngü kendiliğinden biter.
    fn watch_pointer(&mut self, cx: &mut Context<Self>) {
        if self.pointer_watch.is_some() {
            return;
        }
        let Some(window) = cx.windows().first().copied() else {
            return;
        };
        self.pointer_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POINTER_POLL).await;
                let keep_going = window
                    .update(cx, |_, window, cx| {
                        // X11: imleç konumu doğrudan sorgulanır (köşelere çıkışı da yakalar).
                        // XWayland: imleç yerel bir Wayland yüzeyine geçince X sunucusu son
                        // konumu bildirmeye devam eder; orada çıkış olayı ise güvenilir.
                        // Wayland: konum sorgulanamaz; pencereden çıkış olayı güvenilir.
                        // Daireden köşelere çıkışı orada `on_mouse_move` yakalar.
                        let pos = platform::pointer_position(window);
                        let inside = match pos {
                            Some(pos) if platform::is_xwayland() => {
                                in_circle(pos, window) && window.is_window_hovered()
                            }
                            Some(pos) => in_circle(pos, window),
                            None => window.is_window_hovered(),
                        };
                        log::trace!("poll {pos:?} inside={inside}");
                        this.update(cx, |this, cx| {
                            if !inside {
                                this.set_hovered(false, cx);
                            }
                            let watching = this.controls.visible;
                            if !watching {
                                this.pointer_watch = None;
                            }
                            watching
                        })
                        .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        }));
    }

    // ---- Görünüm parçaları ---------------------------------------------------

    fn key_context(&self) -> &'static str {
        match (self.mode, &self.meeting) {
            (Mode::Pomodoro, _) => "Widget Pomodoro",
            (Mode::Meeting, Some(_)) => "Widget Meeting",
            (Mode::Meeting, None) => "Widget MeetingInput",
        }
    }

    /// Hover'da üstte açılan satır: ayarlar ve diğer moda geçiş.
    fn top_row(&self, reveal: f32, cx: &mut Context<Self>) -> Div {
        let k = self.scale;
        let target = match self.mode {
            Mode::Pomodoro => icon::MEETING,
            Mode::Meeting => icon::POMODORO,
        };
        div()
            .h(px(TOP_HEIGHT * k * reveal))
            .overflow_hidden()
            .opacity(reveal)
            .flex()
            .items_start()
            .gap(px(2.0 * k))
            .child(
                icon_button("settings", icon::SETTINGS, 14.0, k, theme::muted())
                    .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
            )
            .child(
                icon_button("mode", target, 14.0, k, theme::muted())
                    .on_click(cx.listener(|this, _, _, cx| this.switch_mode(cx))),
            )
    }

    fn pomodoro_view(&self, now: Instant, reveal: f32, cx: &mut Context<Self>) -> Content {
        let timer = self.pomodoro.timer();
        let state = timer.state();
        let color = theme::phase(self.pomodoro.phase());
        let (filled, total) = self.pomodoro.rounds();
        let k = self.scale;

        let dots = div().flex().items_center().gap(px(6.0 * k)).children((0..total).map(|i| {
            let dot = div().size(px(6.0 * k)).rounded_full();
            if i < filled { dot.bg(color) } else { dot.bg(theme::ring_track()) }
        }));

        let (play_icon, play_color) = match state {
            TimerState::Running => (icon::PAUSE, theme::text()),
            _ => (icon::PLAY, color),
        };
        let controls = div()
            .flex()
            .items_center()
            .gap(px(4.0 * k))
            .child(
                icon_button("reset", icon::RESET, 15.0, k, theme::muted())
                    .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
            )
            .child(
                icon_button("toggle", play_icon, 20.0, k, play_color)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .child(
                icon_button("skip", icon::SKIP, 15.0, k, theme::muted())
                    .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
            );

        Content {
            ring_color: color,
            fraction: 1.0 - timer.progress(now),
            main: clock(
                timer.remaining(now),
                reveal,
                k,
                match state {
                    TimerState::Paused => theme::muted(),
                    _ => theme::text(),
                },
            )
            .into_any_element(),
            info: Some(dots.into_any_element()),
            controls: Some(controls.into_any_element()),
        }
    }

    fn meeting_view(
        &self,
        meeting: &Meeting,
        now: Instant,
        reveal: f32,
        cx: &mut Context<Self>,
    ) -> Content {
        let stage = meeting.stage(now);
        let remaining = meeting.remaining(now);
        let end = SharedString::from(meeting.ends_at().format("%H:%M").to_string());
        let clock_color = match stage {
            Stage::Overtime => theme::stage(Stage::Overtime),
            _ => theme::text(),
        };
        let k = self.scale;
        let small = |text: SharedString| {
            div().text_size(px(13.0 * k)).text_color(theme::muted()).child(text)
        };

        let (main, info) = match self.meeting_settings.display {
            MeetingDisplay::Remaining => (clock(remaining, reveal, k, clock_color), None),
            MeetingDisplay::EndTime => (big_text(end, reveal, k, clock_color), None),
            MeetingDisplay::Both => {
                (clock(remaining, reveal, k, clock_color), Some(small(format!("→ {end}").into())))
            }
        };
        let controls = div().flex().items_center().child(
            icon_button("end", icon::STOP, 15.0, k, theme::muted())
                .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
        );

        Content {
            ring_color: theme::stage(stage),
            // Süre aşılınca halka boşalmak yerine tamamen kırmızı olur.
            fraction: match stage {
                Stage::Overtime => 1.0,
                _ => 1.0 - meeting.timer().progress(now),
            },
            main: main.into_any_element(),
            info: info.map(IntoElement::into_any_element),
            controls: Some(controls.into_any_element()),
        }
    }

    fn meeting_input_view(&self, window: &Window) -> Content {
        let k = self.scale;
        let focused = self.focus.is_focused(window);
        let text = if self.input.is_empty() {
            div().text_color(theme::muted()).child("45 · 14:30")
        } else {
            div().text_color(theme::text()).child(SharedString::from(self.input.clone()))
        };
        let caret = div().w(px(2.0)).h(px(20.0 * k)).rounded_sm().bg(theme::stage(Stage::Normal));
        let field = div()
            .w(px(120.0 * k))
            .h(px(36.0 * k))
            .rounded(px(10.0 * k))
            .bg(theme::button_hover())
            .flex()
            .items_center()
            .justify_center()
            .gap(px(1.0))
            .text_size(px(19.0 * k))
            .child(text)
            .when(focused && !self.input.is_empty(), |d| d.child(caret));
        let hint = match &self.input_error {
            Some(error) => div().text_color(theme::error()).child(error.clone()),
            None => div().text_color(theme::muted()).child("süre ya da saat  ↵"),
        };

        Content {
            ring_color: theme::stage(Stage::Normal),
            fraction: 1.0,
            main: div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.0 * k))
                .child(field)
                .child(hint.text_size(px(11.0 * k)))
                .into_any_element(),
            info: None,
            controls: None,
        }
    }
}

/// Bir modun dairede gösterdikleri.
struct Content {
    ring_color: Hsla,
    /// Halkanın dolu oranı (kalan süre).
    fraction: f32,
    main: gpui::AnyElement,
    /// Normalde altta görünen satır; hover'da kontrollere yer açar.
    info: Option<gpui::AnyElement>,
    /// Hover'da altta açılan kontroller.
    controls: Option<gpui::AnyElement>,
}

fn big_text(text: SharedString, reveal: f32, k: f32, color: Hsla) -> Div {
    // "1:30:00" ya da "+12:00" gibi uzun metinler halkaya yaklaşmasın.
    let scale = match text.chars().count() {
        0..=5 => 1.0,
        6 => 0.88,
        _ => 0.76,
    };
    div()
        .text_size(px((CLOCK_SIZE + (CLOCK_SIZE_HOVER - CLOCK_SIZE) * reveal) * scale * k))
        .text_color(color)
        .child(text)
}

fn clock(remaining: Remaining, reveal: f32, k: f32, color: Hsla) -> Div {
    let text = match remaining {
        Remaining::Left(d) => format_clock(d),
        Remaining::Over(d) => format!("+{}", format_clock(d)),
    };
    big_text(text.into(), reveal, k, color)
}

/// Config'deki Pomodoro ayarları. Debug derlemesinde `POMODORO_DEMO=1` ile
/// süreler dakika yerine saniye olur (test için).
fn pomodoro_settings(config: &Config) -> PomodoroSettings {
    let settings = config.pomodoro_settings();
    #[cfg(debug_assertions)]
    if std::env::var_os("POMODORO_DEMO").is_some() {
        let secs = |d: Duration| Duration::from_secs(d.as_secs() / 60);
        return PomodoroSettings {
            work: secs(settings.work),
            short_break: secs(settings.short_break),
            long_break: secs(settings.long_break),
            ..settings
        };
    }
    settings
}

/// Gösterilen saniyenin değişmesine kalan süre (+ küçük bir pay).
fn until_next_second(remaining: Remaining) -> Duration {
    const SLACK: Duration = Duration::from_millis(15);
    let nanos = match remaining {
        // Kalan süre yukarı yuvarlanarak gösteriliyor; kesir sıfırlanınca değişir.
        Remaining::Left(d) => d.subsec_nanos(),
        Remaining::Over(d) => 1_000_000_000 - d.subsec_nanos(),
    };
    Duration::from_nanos(nanos.into()) + SLACK
}

/// Yuvarlak, hover'da hafifçe vurgulanan ikon düğmesi.
fn icon_button(
    id: &'static str,
    path: &'static str,
    size: f32,
    k: f32,
    color: Hsla,
) -> Stateful<Div> {
    let size = size * k;
    let box_size = size + 14.0 * k;
    div()
        .id(id)
        .size(px(box_size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(theme::button_hover()))
        .child(svg().path(path).size(px(size)).text_color(color))
}

impl Render for Widget {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let reveal = self.controls.value();
        let k = self.scale;

        let content = match (self.mode, &self.meeting) {
            (Mode::Pomodoro, _) => self.pomodoro_view(now, reveal, cx),
            (Mode::Meeting, Some(meeting)) => self.meeting_view(meeting, now, reveal, cx),
            (Mode::Meeting, None) => self.meeting_input_view(window),
        };

        // Alt satırlar: normalde bilgi, hover'da kontroller. Yükseklikleri de
        // geçişe dahil; böylece içerik her durumda dikeyde ortalı kalır.
        let info = content.info.map(|info| {
            div()
                .h(px(INFO_HEIGHT * k * (1.0 - reveal)))
                .overflow_hidden()
                .opacity(1.0 - reveal)
                .flex()
                .items_center()
                .child(info)
        });
        let controls = content.controls.map(|controls| {
            div()
                .h(px(CONTROLS_HEIGHT * k * reveal))
                .overflow_hidden()
                .opacity(reveal)
                .flex()
                .items_center()
                .child(controls)
        });

        // Pencerenin tamamı transparan; sadece daire boyanır. Hover kararı
        // kare pencereye göre değil, dairenin kendisine göre verilir.
        div()
            .id("root")
            .size_full()
            .track_focus(&self.focus)
            .key_context(self.key_context())
            .on_action(cx.listener(|this, _: &ToggleTimer, _, cx| this.toggle(cx)))
            .on_action(cx.listener(|this, _: &ResetTimer, _, cx| this.reset(cx)))
            .on_action(cx.listener(|this, _: &SkipPhase, _, cx| this.skip(cx)))
            .on_action(cx.listener(|this, _: &SwitchMode, _, cx| this.switch_mode(cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.open_settings(cx)))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if this.mode == Mode::Meeting
                    && this.meeting.is_none()
                    && this.input_key(&e.keystroke, cx)
                {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
                this.set_hovered(in_circle(e.position, window), cx);
            }))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(div().absolute().inset_0().child(ring(
                        self.ring.clone(),
                        content.fraction,
                        RingStyle {
                            width: RING_WIDTH * k,
                            surface: theme::surface(),
                            track: theme::ring_track(),
                            color: content.ring_color,
                        },
                    )))
                    .child(self.top_row(reveal, cx))
                    .child(content.main)
                    .children(info)
                    .children(controls),
            )
    }
}
