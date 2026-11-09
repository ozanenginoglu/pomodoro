//! Daire widget'ı: Pomodoro ve toplantı modları, halka, hover'da açılan kontroller.

use std::time::{Duration, Instant};

use gpui::{
    App, Context, Div, Entity, FocusHandle, Hsla, InteractiveElement, IntoElement, KeyBinding,
    KeyDownEvent, Keystroke, MouseButton, MouseMoveEvent, ParentElement, Pixels, Point, Render,
    SharedString, Stateful, Styled, Subscription, Task, Window, WindowControlArea, WindowHandle,
    actions, div, prelude::*, px, size, svg,
};

use super::{
    eye::{self, Eye, Mood, Pose, PoseKey, SharedEyeCache},
    ring::{RingStyle, SharedRingCache, ring},
    settings::{self, SettingsPanel},
    theme,
};
use crate::{
    alert::{self, Alert, Chime},
    assets::icon,
    camera,
    config::{AppearanceConfig, CameraConfig, Config, NotificationConfig, Position},
    i18n::{self, t},
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
/// Göz görünürken yeniden çizim aralığı (~30 fps; yavaş hareketler için yeterli).
const EYE_FRAME: Duration = Duration::from_millis(33);
/// Açılamayan ya da görüntüsü kesilen kamera bu aralıkla yeniden denenir.
const CAMERA_RETRY: Duration = Duration::from_secs(10);
/// Mola korunurken kullanıcının bilgisayar başında olup olmadığını yoklama aralığı.
const PRESENCE_POLL: Duration = Duration::from_millis(250);

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
/// Mola bekçisi domatesin alanı ve altındaki küçük saat.
const EYE_WIDTH: f32 = eye::VIEW[0] * 0.9;
const EYE_HEIGHT: f32 = eye::VIEW[1] * 0.9;
const EYE_CLOCK_SIZE: f32 = 24.0;
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
    /// Kullanıcı bilgisayar başında mı? Mola korunurken yoklanır; ne kamera
    /// ne de platform bilemiyorsa `None` (göz görünmez, mola her zamanki gibi işler).
    activity: Option<bool>,
    next_presence_poll: Instant,
    camera_config: CameraConfig,
    /// Kamera; yalnızca mola korunurken ve ayar açıkken açık.
    camera: Option<camera::Watch>,
    camera_started: Instant,
    /// Kameranın son gördüğü yüzün merkezi; domates ona bakar.
    face: Option<[f32; 2]>,
    /// Mola bekçisi domates; yalnızca mola korunurken var.
    eye: Option<Eye>,
    /// En son çizilen domates pozu; değişmedikçe yeniden çizilmez.
    drawn_eye: Option<PoseKey>,
    eye_image: SharedEyeCache,
    /// Uygulanmış görünüm ayarları (konum değişikliğini fark etmek için).
    appearance: AppearanceConfig,
    /// Wayland'de konum ancak yeniden açılışta değişir; sayaçlar dururken yapılır.
    pending_restart: bool,
    /// Kullanıcı widget'ı taşıyınca yeni konumu biraz bekleyip kaydeder.
    save_position: Option<Task<()>>,
    /// Toplantı süresi/saati girişi.
    input: String,
    /// Metin olarak değil tür olarak saklanır; her çizimde etkin dilde yazılır.
    input_error: Option<meeting::ParseError>,
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
    _subscriptions: [Subscription; 2],
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
        // Widget sürüklenince (Windows) ya da pencere yöneticisiyle taşınınca konumu hatırla.
        let bounds_subscription =
            cx.observe_window_bounds(window, |this, window, cx| this.window_moved(window, cx));
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
            activity: None,
            next_presence_poll: Instant::now(),
            camera_config: c.camera.clone(),
            camera: None,
            camera_started: Instant::now(),
            face: None,
            eye: None,
            drawn_eye: None,
            eye_image: SharedEyeCache::default(),
            appearance: c.appearance.clone(),
            pending_restart: false,
            save_position: None,
            input: String::new(),
            input_error: None,
            focus,
            controls: Fade::new(),
            animation: None,
            pointer_watch: None,
            ticker: None,
            ring: SharedRingCache::default(),
            status,
            _subscriptions: [subscription, bounds_subscription],
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
            Command::Quit => cx.quit(),
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
                    paused: state == TimerState::Paused && !self.pomodoro.is_held(),
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
        // Ayarlar paneli dili zaten uygulamış olabilir; burada yalnızca eşitleriz.
        // Widget aşağıdaki `changed` ile yeniden çizilir.
        i18n::set(config.language);
        self.pomodoro.set_settings(pomodoro_settings(config));
        self.meeting_settings = config.meeting_settings();
        self.notifications = config.notifications.clone();
        if config.camera != self.camera_config {
            // Kamera değiştiyse ya da kapatıldıysa, gerekiyorsa sonraki yoklamada yeniden açılır.
            self.camera_config = config.camera.clone();
            self.camera = None;
            self.face = None;
        }
        if !self.camera_config.enabled {
            self.pomodoro.set_user_present(false, Instant::now());
        }
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

    /// Pencere ayarlardaki yerinden taşındıysa, hareket bitince yeni konumu
    /// `custom` olarak kaydeder. Kendi yaptığımız taşımalar beklenen konuma
    /// denk geldiği için sayılmaz.
    fn window_moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !platform::remembers_position(cx) {
            return;
        }
        let expected = platform::widget_bounds(cx, &self.appearance).origin;
        let actual = window.bounds().origin;
        if !platform::moved_by_user(expected, actual) {
            self.save_position = None;
            return;
        }
        let config = self.config.clone();
        self.save_position = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let position = [f32::from(actual.x).round() as i32, f32::from(actual.y).round() as i32];
            let _ = this.update(cx, |this, cx| {
                this.save_position = None;
                // Yeni konum zaten uygulanmış; `apply_config` pencereyi geri taşımasın.
                this.appearance.position = Position::Custom;
                this.appearance.custom = Some(position);
                config.update(cx, |config, cx| {
                    config.appearance.position = Position::Custom;
                    config.appearance.custom = Some(position);
                    config.save();
                    cx.notify();
                });
            });
        }));
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
            Err(e) => self.input_error = Some(e),
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
        self.ensure_animation(cx);
    }

    // ---- Mola koruması --------------------------------------------------

    /// Bir mola sürüyor (ya da kullanıcı yüzünden tutuluyor) ve ayar açık.
    fn guarding(&self) -> bool {
        self.camera_config.enabled
            && self.pomodoro.phase() != Phase::Work
            && self.pomodoro.is_active()
    }

    /// Göz görünür mü? Kullanıcının etkinliği bilinemiyorsa koruma yoktur.
    fn shows_eye(&self) -> bool {
        self.guarding() && self.activity.is_some()
    }

    /// Kullanıcı bilgisayar başındaysa molayı tutar, uzaklaşınca bırakır.
    /// Animasyon döngüsünden çağrılır.
    fn poll_presence(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        if !self.guarding() {
            self.activity = None;
            // Mola bitti: kamera hemen kapansın.
            self.camera = None;
            self.face = None;
            return;
        }
        if now < self.next_presence_poll {
            return;
        }
        self.next_presence_poll = now + PRESENCE_POLL;
        // Kamera bir yüz görüyorsa ya da klavye/fare kullanılıyorsa bilgisayar
        // başında. Kamera açılırken, açılamadığında ya da görüntü kesildiğinde
        // yalnızca klavye/fareye bakılır.
        let input = platform::user_active();
        let sighting = self.watch_camera();
        self.face = sighting.and_then(|s| s.face);
        self.activity = match sighting {
            Some(s) => Some(s.present || input.unwrap_or(false)),
            None => input,
        };
        // Bilinemiyorsa mola her zamanki gibi işler.
        let present = self.activity.unwrap_or(false);
        if let Some(change) = self.pomodoro.set_user_present(present, now) {
            log::debug!("mola koruması: {change:?}");
            self.publish_status();
            self.ensure_ticker(cx);
            // Baştan başlayan molanın saati hemen görünsün.
            cx.notify();
        }
    }

    /// Kamerayı açar ve son gördüğünü döndürür. Kamera açılamazsa ya da
    /// görüntü kesilirse arada bir yeniden denenir; o sırada klavye/fare yeterli olur.
    fn watch_camera(&mut self) -> Option<camera::Sighting> {
        let now = Instant::now();
        if self.camera.as_ref().is_some_and(|w| w.failed())
            && now.duration_since(self.camera_started) >= CAMERA_RETRY
        {
            self.camera = None;
        }
        if self.camera.is_none() {
            self.camera_started = now;
        }
        let watch = self
            .camera
            .get_or_insert_with(|| camera::Watch::start(self.camera_config.device.clone()));
        if watch.failed() {
            return None;
        }
        watch.sighting()
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
                            // Mola başladıysa göz belirsin.
                            this.ensure_animation(cx);
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
            Phase::ShortBreak => (t().work_done, t().short_break),
            Phase::LongBreak => (t().work_done, t().long_break),
            Phase::Work => (t().break_done, t().work),
        };
        let body = if started {
            i18n::phase_started(next, minutes)
        } else {
            i18n::phase_up_next(next, minutes)
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
        let left = |d: Duration| i18n::meeting_minutes_left(d.as_secs() / 60, &end);
        match stage {
            Stage::Warning => Alert {
                title: t().meeting.into(),
                body: left(s.warning),
                chime: Chime::Gentle,
                urgent: false,
            },
            Stage::Critical => Alert {
                title: t().meeting.into(),
                body: left(s.critical),
                chime: Chime::Phase,
                urgent: false,
            },
            Stage::Overtime | Stage::Normal => Alert {
                title: t().meeting_over.into(),
                body: i18n::meeting_was_due(&end),
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

    fn is_animating(&self) -> bool {
        self.controls.is_animating() || self.guarding()
    }

    /// X11'de pencere odakta değilken ve fare hareket etmezken animasyon
    /// kareleri gelmeyebiliyor. Bu yüzden `request_animation_frame` yerine
    /// geçiş bitene kadar kendi zamanlayıcımızla yeniden çizim istiyoruz.
    /// Mola korunurken de döngü sürer: domates canlıdır ve kullanıcı yoklanır.
    /// Domates hep hafifçe kıpırdadığı için göz kare hızında (~30 fps) yeniden çizilir.
    fn ensure_animation(&mut self, cx: &mut Context<Self>) {
        if self.animation.is_some() || !self.is_animating() {
            return;
        }
        let Some(window) = cx.windows().first().copied() else {
            return;
        };
        self.animation = Some(cx.spawn(async move |this, cx| {
            loop {
                let frame =
                    this.read_with(cx, |this, _| {
                        if this.controls.is_animating() { FRAME } else { EYE_FRAME }
                    })
                    .unwrap_or(FRAME);
                cx.background_executor().timer(frame).await;
                let keep_going = window
                    .update(cx, |_, window, cx| {
                        this.update(cx, |this, cx| {
                            this.poll_presence(cx);
                            let eye_moved = this.step_eye(Instant::now(), window);
                            if eye_moved || this.controls.is_animating() {
                                cx.notify();
                            }
                            let animating = this.is_animating();
                            if !animating {
                                this.animation = None;
                            }
                            animating
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

    /// Gözü bir kare ilerletir. Kullanıcı bilgisayar başındayken imleci izler.
    /// Görünüşü çizilenden farklıysa `true`.
    fn step_eye(&mut self, now: Instant, window: &Window) -> bool {
        if !self.shows_eye() {
            self.eye = None;
            return self.drawn_eye.is_some();
        }
        let mood = if self.pomodoro.is_held() { Mood::Watching } else { Mood::Relaxed };
        // Kamera bir yüz görüyorsa göz ona, yoksa imlece bakar.
        let face = self.face.map(face_direction);
        let cursor = (mood == Mood::Watching && face.is_none())
            .then(|| platform::cursor_position(window))
            .flatten()
            .map(|p| {
                let size = window.viewport_size();
                let side = f32::from(size.width.min(size.height));
                // Domatesin gözleri dairenin ortasının biraz üstünde.
                let dx = f32::from(p.x) - f32::from(size.width) / 2.0;
                let dy = f32::from(p.y) - (f32::from(size.height) / 2.0 - side * 0.04);
                eye::cursor_direction(dx, dy, side)
            });
        let eye = self.eye.get_or_insert_with(|| Eye::new(now));
        eye.update(now, mood, face.or(cursor));
        self.drawn_eye != Some(eye.pose(now).quantized())
    }

    /// Çizilecek göz pozu (hareket `step_eye`'da ilerler).
    fn eye_pose(&mut self, now: Instant) -> Option<Pose> {
        let pose = if self.shows_eye() {
            Some(self.eye.get_or_insert_with(|| Eye::new(now)).pose(now))
        } else {
            self.eye = None;
            None
        };
        self.drawn_eye = pose.map(|p| p.quantized());
        pose
    }

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

    fn pomodoro_view(
        &self,
        now: Instant,
        reveal: f32,
        eye: Option<Pose>,
        cx: &mut Context<Self>,
    ) -> Content {
        let timer = self.pomodoro.timer();
        // Tutulan mola kullanıcıya sürüyor görünür; duraklatılmış değildir.
        let state = match timer.state() {
            TimerState::Paused if self.pomodoro.is_held() => TimerState::Running,
            state => state,
        };
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

        let clock_color = match state {
            TimerState::Paused => theme::muted(),
            _ => theme::text(),
        };
        let main = match eye {
            // Mola korunurken: domates ve altında küçük saat.
            Some(pose) => {
                let shrink = 1.0 - 0.3 * reveal;
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w(px(EYE_WIDTH * k * shrink))
                            .h(px(EYE_HEIGHT * k * shrink))
                            .child(eye::eye(self.eye_image.clone(), pose)),
                    )
                    .child(
                        div()
                            .text_size(px(EYE_CLOCK_SIZE * k))
                            .text_color(clock_color)
                            .child(SharedString::from(remaining_text(timer.remaining(now)))),
                    )
            }
            None => clock(timer.remaining(now), reveal, k, clock_color),
        };

        Content {
            ring_color: color,
            fraction: 1.0 - timer.progress(now),
            main: main.into_any_element(),
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
            Some(error) => div().text_color(theme::error()).child(error.to_string()),
            None => div().text_color(theme::muted()).child(t().meeting_input_hint),
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

fn remaining_text(remaining: Remaining) -> String {
    match remaining {
        Remaining::Left(d) => format_clock(d),
        Remaining::Over(d) => format!("+{}", format_clock(d)),
    }
}

fn clock(remaining: Remaining, reveal: f32, k: f32, color: Hsla) -> Div {
    big_text(remaining_text(remaining).into(), reveal, k, color)
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

/// Kameradaki yüzün konumundan gözün bakış yönü. Kamera kullanıcıya baktığı
/// için görüntü aynalanmamıştır: kullanıcı kendi soluna geçince karenin
/// sağında görünür; göz de (ekranda) sola bakmalı.
fn face_direction([x, y]: [f32; 2]) -> [f32; 2] {
    [(0.5 - x) * 2.0, (y - 0.5) * 1.6].map(|v| v.clamp(-1.0, 1.0))
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
        // Sürükleme alanının (Windows) tıklamayı yutmaması için.
        .occlude()
        .hover(|s| s.bg(theme::button_hover()))
        .child(svg().path(path).size(px(size)).text_color(color))
}

impl Render for Widget {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let reveal = self.controls.value();
        let k = self.scale;
        let eye = self.eye_pose(now);

        let content = match (self.mode, &self.meeting) {
            (Mode::Pomodoro, _) => self.pomodoro_view(now, reveal, eye, cx),
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
            // Windows: dairenin boş alanı pencereyi sürükler (düğmeler hariç).
            .when(platform::DRAG_TO_MOVE, |d| d.window_control_area(WindowControlArea::Drag))
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
