//! Bildirimler ve zil sesi. İkisi de kendi iş parçacığında çalışır; arayüzü
//! asla bekletmez. Ses dosyası yerine çan sesi burada sentezlenir.

use std::{f32::consts::TAU, num::NonZero, thread};

use crate::config::NotificationConfig;

/// Hangi zil sesi çalınsın?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chime {
    /// Tek, yumuşak bir çan: "haberin olsun".
    Gentle,
    /// Yükselen iki nota: evre değişti.
    Phase,
    /// Üç nota, iki kez: süre doldu.
    Alarm,
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub title: String,
    pub body: String,
    pub chime: Chime,
    /// Dunst gibi bildirim sunucularında "kritik" (kendiliğinden kapanmaz).
    pub urgent: bool,
}

/// Bildirimi ve sesi ayarlara göre gönderir/çalar; hemen döner.
pub fn fire(alert: Alert, config: &NotificationConfig) {
    log::info!("uyarı: {} — {}", alert.title, alert.body);
    if config.sound {
        let chime = alert.chime;
        thread::spawn(move || {
            if let Err(e) = play(chime) {
                log::warn!("ses çalınamadı: {e}");
            }
        });
    }
    if config.enabled {
        thread::spawn(move || {
            if let Err(e) = notify(&alert) {
                log::warn!("bildirim gönderilemedi: {e}");
            }
        });
    }
}

fn notify(alert: &Alert) -> Result<(), notify_rust::error::Error> {
    let mut notification = notify_rust::Notification::new();
    notification.appname("Pomodoro").summary(&alert.title).body(&alert.body).icon("alarm-clock");
    #[cfg(not(target_os = "macos"))]
    if alert.urgent {
        notification.urgency(notify_rust::Urgency::Critical);
    }
    notification.show().map(drop)
}

fn play(chime: Chime) -> Result<(), Box<dyn std::error::Error>> {
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink()?;
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());
    let samples = synthesize(chime);
    player.append(rodio::buffer::SamplesBuffer::new(
        NonZero::new(1).unwrap(),
        NonZero::new(SAMPLE_RATE).unwrap(),
        samples,
    ));
    player.sleep_until_end();
    Ok(())
}

const SAMPLE_RATE: u32 = 48_000;

/// (başlangıç sn, frekans Hz) listesi.
fn notes(chime: Chime) -> &'static [(f32, f32)] {
    // E5, G#5, B5: yumuşak bir majör üçlü.
    const E5: f32 = 659.25;
    const GS5: f32 = 830.61;
    const B5: f32 = 987.77;
    match chime {
        Chime::Gentle => &[(0.0, GS5)],
        Chime::Phase => &[(0.0, E5), (0.18, B5)],
        Chime::Alarm => &[(0.0, E5), (0.14, GS5), (0.28, B5), (0.9, E5), (1.04, GS5), (1.18, B5)],
    }
}

/// Çan sesi: temel frekans + iki üst harmonik, hızlı atak ve üstel sönüm.
fn synthesize(chime: Chime) -> Vec<f32> {
    const NOTE_LENGTH: f32 = 1.4;
    const ATTACK: f32 = 0.006;
    const VOLUME: f32 = 0.22;
    let notes = notes(chime);
    let end = notes.iter().map(|(start, _)| start).fold(0.0f32, |a, &b| a.max(b)) + NOTE_LENGTH;
    let len = (end * SAMPLE_RATE as f32) as usize;

    let mut out = vec![0.0f32; len];
    for &(start, freq) in notes {
        let offset = (start * SAMPLE_RATE as f32) as usize;
        for (i, sample) in out[offset..].iter_mut().enumerate() {
            let t = i as f32 / SAMPLE_RATE as f32;
            if t > NOTE_LENGTH {
                break;
            }
            let envelope = (t / ATTACK).min(1.0) * (-t * 4.0).exp();
            let tone = (TAU * freq * t).sin()
                + 0.35 * (TAU * freq * 2.0 * t).sin() * (-t * 3.0).exp()
                + 0.12 * (TAU * freq * 3.0 * t).sin() * (-t * 6.0).exp();
            *sample += VOLUME * envelope * tone / 1.47;
        }
    }
    // Üst üste binen notalar taşmasın.
    let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    if peak > 0.9 {
        out.iter_mut().for_each(|s| *s *= 0.9 / peak);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesized_chimes_are_audible_and_never_clip() {
        for chime in [Chime::Gentle, Chime::Phase, Chime::Alarm] {
            let samples = synthesize(chime);
            let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
            assert!(peak > 0.05 && peak <= 0.9, "{chime:?}: tepe {peak}");
            // Son örnek neredeyse sessiz olmalı (tık sesi olmasın).
            assert!(samples.last().unwrap().abs() < 0.01, "{chime:?}");
        }
    }

    #[test]
    fn alarm_is_longer_than_gentle() {
        assert!(synthesize(Chime::Alarm).len() > synthesize(Chime::Gentle).len());
    }
}
