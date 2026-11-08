//! Küçük, başlık çubuğu olmayan ayarlar paneli. Her değişiklik anında
//! kaydedilir ve widget'a uygulanır; ayrı bir "Kaydet" düğmesi yoktur.
//! `Esc` ile ya da panel odağı kaybedince kapanır.

use std::ops::RangeInclusive;

use gpui::{
    App, AppContext, Bounds, Context, Div, ElementId, Entity, FocusHandle, Hsla, KeyBinding,
    ParentElement, Render, SharedString, Styled, Subscription, Window, WindowHandle, actions, div,
    prelude::*, px, size, svg,
};

use super::theme;
use crate::i18n::{Language, t};
use crate::{
    assets::icon,
    config::{Config, Display, Position, Size, limits},
    platform,
};

actions!(settings, [CloseSettings]);

const CONTEXT: &str = "Settings";
const WIDTH: f32 = 320.0;
/// Windows'ta fazladan "Oturum açılınca başlat" satırı var.
const HEIGHT: f32 = if cfg!(windows) { 692.0 } else { 658.0 };

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", CloseSettings, Some(CONTEXT))]);
}

/// Paneli açar; zaten açıksa öne getirir.
pub fn open(
    existing: Option<WindowHandle<SettingsPanel>>,
    config: Entity<Config>,
    cx: &mut App,
) -> Option<WindowHandle<SettingsPanel>> {
    if let Some(handle) = existing
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return Some(handle);
    }
    let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
    platform::open_settings(cx, bounds, move |window, cx| {
        let config = config.clone();
        cx.new(|cx| SettingsPanel::new(config, window, cx))
    })
    .inspect_err(|e| log::error!("ayarlar penceresi açılamadı: {e}"))
    .ok()
}

pub struct SettingsPanel {
    config: Entity<Config>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsPanel {
    fn new(config: Entity<Config>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subscriptions = vec![
            cx.observe(&config, |_, _, cx| cx.notify()),
            // Panelin dışına tıklanınca (odak gidince) kapan.
            cx.observe_window_activation(window, |_, window, _| {
                if !window.is_window_active() {
                    window.remove_window();
                }
            }),
        ];
        Self { config, focus, _subscriptions: subscriptions }
    }

    /// Ayarı değiştirir, sınırlara kırpar, kaydeder ve widget'a haber verir.
    fn set(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Config)) {
        self.config.update(cx, |config, cx| {
            let mut next = config.clone();
            change(&mut next);
            let next = next.normalized();
            if next != *config {
                // Dil değiştiyse panel de aynı karede yeni dille çizilsin.
                crate::i18n::set(next.language);
                *config = next;
                config.save();
                cx.notify();
            }
        });
    }

    fn stepper(&self, spec: Stepper, value: u32, cx: &mut Context<Self>) -> Div {
        let Stepper { key, label, unit, range, field } = spec;
        let (min, max) = (*range.start(), *range.end());
        let button = |id: usize, text: &'static str, enabled: bool, delta: i64| {
            div()
                .id(ElementId::from((key, id)))
                .size(px(26.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(16.0))
                .text_color(if enabled { theme::text() } else { theme::ring_track() })
                .when(enabled, |d| {
                    d.cursor_pointer().hover(|s| s.bg(theme::button_hover())).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.set(cx, |c| {
                                let v = field(c);
                                *v = (i64::from(*v) + delta).clamp(min.into(), max.into()) as u32;
                            })
                        },
                    ))
                })
                .child(text)
        };
        // Uzun süreler için büyük adım: 60 dakikanın üstünde 5'er dakika
        // (tur sayısı en fazla 12 olduğu için yalnızca süreleri etkiler).
        let step = if value >= 60 { 5 } else { 1 };
        row(label).child(
            div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(button(0, "−", value > min, -step))
                .child(
                    div()
                        .w(px(64.0))
                        .flex()
                        .justify_center()
                        .text_color(theme::text())
                        .child(SharedString::from(format!("{value} {unit}"))),
                )
                .child(button(1, "+", value < max, step)),
        )
    }

    fn toggle(
        &self,
        key: &'static str,
        label: &'static str,
        on: bool,
        field: fn(&mut Config) -> &mut bool,
        cx: &mut Context<Self>,
    ) -> Div {
        switch(
            key,
            label,
            on,
            cx.listener(move |this, _, _, cx| this.set(cx, |c| *field(c) = !*field(c))),
        )
    }

    /// Oturum açılınca başlatma (yalnızca Windows). Ayar dosyasında değil,
    /// sistemde (kayıt defteri) tutulur.
    fn autostart_row(&self, cx: &mut Context<Self>) -> Option<Div> {
        let on = platform::autostart()?;
        Some(switch(
            "autostart",
            t().start_on_sign_in,
            on,
            cx.listener(move |_, _, _, cx| {
                if let Err(e) = platform::set_autostart(!on) {
                    log::error!("açılışta başlatma ayarlanamadı: {e}");
                }
                cx.notify();
            }),
        ))
    }

    fn segmented<T: Copy + PartialEq + 'static>(
        &self,
        key: &'static str,
        label: &'static str,
        options: &[(T, Label)],
        selected: T,
        apply: fn(&mut Config, T),
        cx: &mut Context<Self>,
    ) -> Div {
        row(label).child(
            div()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(8.0))
                .bg(theme::button_hover())
                .children(options.iter().enumerate().map(|(i, &(value, label))| {
                    let active = value == selected;
                    let color = if active { theme::text() } else { theme::muted() };
                    div()
                        .id(ElementId::from((key, i)))
                        .px(px(9.0))
                        .py(px(3.0))
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_color(color)
                        .when(active, |d| d.bg(theme::surface()))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.set(cx, |c| apply(c, value))),
                        )
                        .child(match label {
                            Label::Text(text) => div().child(text),
                            // Metin satırıyla aynı yükseklikte dursun.
                            Label::Icon(path) => div()
                                .h(px(19.0))
                                .flex()
                                .items_center()
                                .child(svg().path(path).size(px(14.0)).text_color(color)),
                        })
                })),
        )
    }
}

/// Açma/kapama anahtarı satırı.
fn switch(
    key: &'static str,
    label: &'static str,
    on: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let knob = div().size(px(16.0)).rounded_full().bg(theme::text());
    row(label).child(
        div()
            .id(key)
            .w(px(38.0))
            .h(px(22.0))
            .p(px(3.0))
            .rounded_full()
            .cursor_pointer()
            .flex()
            .items_center()
            .when(on, |d| d.justify_end().bg(theme::accent()))
            .when(!on, |d| d.justify_start().bg(theme::button_hover()))
            .on_click(on_click)
            .child(knob),
    )
}

/// Seçim düğmesinin içeriği.
#[derive(Clone, Copy)]
enum Label {
    Text(&'static str),
    Icon(&'static str),
}

use Label::{Icon, Text};

/// Bir "−  25 dk  +" satırının tanımı.
struct Stepper {
    key: &'static str,
    label: &'static str,
    unit: &'static str,
    range: RangeInclusive<u32>,
    field: fn(&mut Config) -> &mut u32,
}

const fn stepper(
    key: &'static str,
    label: &'static str,
    unit: &'static str,
    range: RangeInclusive<u32>,
    field: fn(&mut Config) -> &mut u32,
) -> Stepper {
    Stepper { key, label, unit, range, field }
}

fn row(label: &'static str) -> Div {
    div()
        .h(px(34.0))
        .flex()
        .items_center()
        .justify_between()
        .child(div().text_color(theme::text()).child(label))
}

fn section(title: &'static str) -> Div {
    div().pt(px(14.0)).pb(px(2.0)).text_size(px(11.0)).text_color(theme::muted()).child(title)
}

fn close_button(cx: &mut Context<SettingsPanel>) -> impl IntoElement {
    div()
        .id("close")
        .size(px(28.0))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(theme::button_hover()))
        .on_click(cx.listener(|_, _, window, _| window.remove_window()))
        .child(svg().path(icon::CLOSE).size(px(14.0)).text_color(theme::muted()))
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.config.read(cx).clone();
        let (p, m, n) = (&c.pomodoro, &c.meeting, &c.notifications);
        let border: Hsla = theme::ring_track();

        div()
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(|_, _: &CloseSettings, window, _| window.remove_window()))
            .size_full()
            .p(px(1.0))
            .child(
                div()
                    .size_full()
                    .rounded(px(platform::SETTINGS_RADIUS))
                    .border_1()
                    .border_color(border)
                    .bg(theme::surface())
                    .px(px(20.0))
                    .py(px(14.0))
                    .text_size(px(13.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .text_color(theme::text())
                                    .child(t().settings),
                            )
                            .child(close_button(cx)),
                    )
                    .child(section(t().section_pomodoro))
                    .child(self.stepper(
                        stepper("work", t().work, t().unit_minutes, limits::WORK, |c| {
                            &mut c.pomodoro.work_minutes
                        }),
                        p.work_minutes,
                        cx,
                    ))
                    .child(self.stepper(
                        stepper("short", t().short_break, t().unit_minutes, limits::BREAK, |c| {
                            &mut c.pomodoro.short_break_minutes
                        }),
                        p.short_break_minutes,
                        cx,
                    ))
                    .child(self.stepper(
                        stepper("long", t().long_break, t().unit_minutes, limits::BREAK, |c| {
                            &mut c.pomodoro.long_break_minutes
                        }),
                        p.long_break_minutes,
                        cx,
                    ))
                    .child(self.stepper(
                        stepper("rounds", t().rounds_before_long_break, t().unit_rounds, limits::ROUNDS, |c| {
                            &mut c.pomodoro.rounds_before_long_break
                        }),
                        p.rounds_before_long_break,
                        cx,
                    ))
                    .child(self.toggle(
                        "auto-break",
                        t().auto_start_breaks,
                        p.auto_start_breaks,
                        |c| &mut c.pomodoro.auto_start_breaks,
                        cx,
                    ))
                    .child(self.toggle(
                        "auto-work",
                        t().auto_start_work,
                        p.auto_start_work,
                        |c| &mut c.pomodoro.auto_start_work,
                        cx,
                    ))
                    .child(section(t().section_meeting))
                    .child(self.segmented(
                        "display",
                        t().display,
                        &[
                            (Display::Remaining, Text(t().display_remaining)),
                            (Display::EndTime, Text(t().display_end_time)),
                            (Display::Both, Text(t().display_both)),
                        ],
                        m.display,
                        |c, v| c.meeting.display = v,
                        cx,
                    ))
                    .child(self.stepper(
                        stepper("warn", t().warning, t().unit_minutes, limits::WARNING, |c| {
                            &mut c.meeting.warning_minutes
                        }),
                        m.warning_minutes,
                        cx,
                    ))
                    .child(self.stepper(
                        stepper("crit", t().critical, t().unit_minutes, limits::CRITICAL, |c| {
                            &mut c.meeting.critical_minutes
                        }),
                        m.critical_minutes,
                        cx,
                    ))
                    .child(section(t().section_notifications))
                    .child(self.toggle(
                        "notify",
                        t().notifications,
                        n.enabled,
                        |c| &mut c.notifications.enabled,
                        cx,
                    ))
                    .child(self.toggle("sound", t().sound, n.sound, |c| &mut c.notifications.sound, cx))
                    .child(section(t().section_appearance))
                    .child(self.segmented(
                        "size",
                        t().size,
                        &[
                            (Size::Small, Text("S")),
                            (Size::Medium, Text("M")),
                            (Size::Large, Text("L")),
                        ],
                        c.appearance.size,
                        |c, v| c.appearance.size = v,
                        cx,
                    ))
                    .child(self.segmented(
                        "position",
                        t().position,
                        &[
                            (Position::TopLeft, Icon(icon::TOP_LEFT)),
                            (Position::TopRight, Icon(icon::TOP_RIGHT)),
                            (Position::Center, Icon(icon::CENTER)),
                            (Position::BottomLeft, Icon(icon::BOTTOM_LEFT)),
                            (Position::BottomRight, Icon(icon::BOTTOM_RIGHT)),
                        ],
                        c.appearance.position,
                        |c, v| c.appearance.position = v,
                        cx,
                    ))
                    .child(self.toggle(
                        "on-top",
                        t().always_on_top,
                        c.appearance.always_on_top,
                        |c| &mut c.appearance.always_on_top,
                        cx,
                    ))
                    .children(self.autostart_row(cx))
                    .child(self.segmented(
                        "language",
                        t().language,
                        &[
                            (Language::English, Text(Language::English.native_name())),
                            (Language::Turkish, Text(Language::Turkish.native_name())),
                        ],
                        c.language,
                        |c, v| c.language = v,
                        cx,
                    )),
            )
    }
}
