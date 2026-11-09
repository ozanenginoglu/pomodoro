//! Mola bekçisi domates.
//!
//! Mola sürerken dairede küçük bir domates belirir:
//! - Kullanıcı uzaktayken (mola işliyor) memnundur: hafifçe sallanır,
//!   yaprakları oynar, gözleri biraz kısık, ara sıra etrafa bakınır.
//! - Kullanıcı bilgisayar başındayken (mola tutuluyor) gözlerini açar, imleci
//!   izler ve birkaç saniye sonra sinirlenir: kıpkırmızı kesilir, kaşlarını
//!   çatar, dişlerini sıkar, yaprakları dikleşip titrer, tepesinden buhar çıkar.
//!
//! Hareket modeli (`Eye`) zamanı parametre olarak alır ve test edilir; çizim,
//! halka gibi `tiny-skia` ile gerçek piksel çözünürlüğünde yapılır.

use std::{cell::RefCell, f32::consts::PI, rc::Rc, sync::Arc, time::Duration, time::Instant};

use gpui::{Bounds, Corners, Hsla, IntoElement, RenderImage, Styled, canvas, size};
use tiny_skia::{FillRule, LineCap, LineJoin, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

use super::{
    raster::{ImageCache, paint, to_image},
    theme,
};

/// Domatesin ruh hali.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    /// Kullanıcı uzakta, mola işliyor.
    Relaxed,
    /// Kullanıcı bilgisayar başında, mola tutuluyor.
    Watching,
}

/// Çizilecek an.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Gözlerin açıklığı, 0 (kapalı) ..= 1.
    pub open: f32,
    /// Bakış yönü, her eksende -1 ..= 1 (y aşağı).
    pub look: [f32; 2],
    /// Sinir, 0 ..= 1.
    pub anger: f32,
    /// Domates belireli beri geçen süre (saniye); sallanma, titreme ve buhar
    /// bununla ilerler.
    pub time: f32,
}

/// Aynıysa yeniden çizmeye gerek olmayan poz anahtarı.
pub type PoseKey = [i32; 5];

impl Pose {
    /// Gözle fark edilmeyecek kadar küçük değişiklikleri yok sayan karşılaştırma
    /// anahtarı. Zaman, göz kare hızına (~30 fps) yuvarlanır.
    pub fn quantized(&self) -> PoseKey {
        let q = |v: f32| (v * 512.0).round() as i32;
        [q(self.open), q(self.look[0]), q(self.look[1]), q(self.anger), (self.time * 30.0) as i32]
    }
}

const BLINK: Duration = Duration::from_millis(160);
/// Bu kadar süre bilgisayar başında kalınca sinirlenmeye başlar…
const ANGER_AFTER: Duration = Duration::from_secs(4);
/// …ve bu sürede tamamen sinirlenir.
const ANGER_RAMP: Duration = Duration::from_secs(3);
/// Uzaktayken gözler biraz kısık: memnun bir bakış.
const RELAXED_OPEN: f32 = 0.8;

pub struct Eye {
    open: f32,
    look: [f32; 2],
    anger: f32,
    target_look: [f32; 2],
    next_glance: Instant,
    blink_start: Option<Instant>,
    next_blink: Instant,
    watching_since: Option<Instant>,
    born: Instant,
    last: Instant,
    rng: fastrand::Rng,
}

impl Eye {
    /// Gözler kapalı başlar; ilk güncellemelerle açılır.
    pub fn new(now: Instant) -> Self {
        Self::with_rng(now, fastrand::Rng::new())
    }

    fn with_rng(now: Instant, mut rng: fastrand::Rng) -> Self {
        let next_blink = now + random_between(&mut rng, 2.5, 6.0);
        Self {
            open: 0.0,
            look: [0.0; 2],
            anger: 0.0,
            target_look: [0.0; 2],
            next_glance: now + Duration::from_millis(800),
            blink_start: None,
            next_blink,
            watching_since: None,
            born: now,
            last: now,
            rng,
        }
    }

    /// Her karede çağrılır. `cursor`: imlecin gözlerden yönü (-1 ..= 1); yoksa
    /// domates kullanıcıya, yani düz ileri bakar.
    pub fn update(&mut self, now: Instant, mood: Mood, cursor: Option<[f32; 2]>) {
        // Uzun bir aradan (askı, gizli pencere) sonra tek adımda sıçramasın.
        let dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;

        let (open, anger, look, look_rate) = match mood {
            Mood::Relaxed => {
                self.watching_since = None;
                if now >= self.next_glance {
                    self.target_look = self.random_glance();
                    self.next_glance = now + random_between(&mut self.rng, 1.2, 3.5);
                }
                // Göz bir yerden ötekine hızla sıçrar (sakkad).
                (RELAXED_OPEN, 0.0, self.target_look, 14.0)
            }
            Mood::Watching => {
                let since = *self.watching_since.get_or_insert(now);
                let watched = now.saturating_duration_since(since).saturating_sub(ANGER_AFTER);
                let anger = (watched.as_secs_f32() / ANGER_RAMP.as_secs_f32()).min(1.0);
                let look = cursor.map_or([0.0; 2], |c| c.map(|v| v.clamp(-1.0, 1.0)));
                (1.0, anger, look, 10.0)
            }
        };
        self.open = approach(self.open, open, 6.0, dt);
        self.anger = approach(self.anger, anger, 4.0, dt);
        for (value, target) in self.look.iter_mut().zip(look) {
            *value = approach(*value, target, look_rate, dt);
        }

        if now >= self.next_blink {
            self.blink_start = Some(now);
            self.next_blink = now + BLINK + random_between(&mut self.rng, 2.5, 6.0);
        }
    }

    pub fn pose(&self, now: Instant) -> Pose {
        let blink = self.blink_start.map_or(1.0, |start| {
            let t = now.saturating_duration_since(start).as_secs_f32() / BLINK.as_secs_f32();
            if t < 1.0 { 1.0 - (PI * t).sin() } else { 1.0 }
        });
        Pose {
            open: self.open * blink,
            look: self.look,
            anger: self.anger,
            time: now.saturating_duration_since(self.born).as_secs_f32(),
        }
    }

    /// Rastgele bir bakış; arada bir düz ileri.
    fn random_glance(&mut self) -> [f32; 2] {
        if self.rng.f32() < 0.25 {
            return [0.0; 2];
        }
        let angle = self.rng.f32() * 2.0 * PI;
        let radius = 0.4 + 0.5 * self.rng.f32();
        [radius * angle.cos(), radius * angle.sin() * 0.6]
    }
}

fn random_between(rng: &mut fastrand::Rng, min: f32, max: f32) -> Duration {
    Duration::from_secs_f32(min + (max - min) * rng.f32())
}

/// Hedefe üstel yaklaşma; `rate` büyüdükçe hızlanır, kare hızından bağımsız.
fn approach(value: f32, target: f32, rate: f32, dt: f32) -> f32 {
    value + (target - value) * (1.0 - (-rate * dt).exp())
}

/// İmlecin gözlere göre yönü: yakındayken küçük, uzaklaştıkça 1'e yaklaşır.
/// `dx`, `dy`: imlecin gözlerin ortasına uzaklığı; `reach`: yarı yola gelinen mesafe.
pub fn cursor_direction(dx: f32, dy: f32, reach: f32) -> [f32; 2] {
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < f32::EPSILON {
        return [0.0; 2];
    }
    let strength = distance / (distance + reach.max(1.0));
    [dx / distance * strength, dy / distance * strength]
}

// ---- Çizim -------------------------------------------------------------

/// Bir önceki domatesi saklar; poz değişmedikçe yeniden çizmez.
#[derive(Default)]
pub struct EyeCache(ImageCache<Key>);

pub type SharedEyeCache = Rc<RefCell<EyeCache>>;

#[derive(Clone, Copy, PartialEq)]
struct Key {
    pixels: [u32; 2],
    pose: PoseKey,
}

/// Domatesi bulunduğu alana çizer.
pub fn eye(cache: SharedEyeCache, pose: Pose) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let scale = window.scale_factor();
            let pixels = [
                (f32::from(bounds.size.width) * scale).round() as u32,
                (f32::from(bounds.size.height) * scale).round() as u32,
            ];
            if pixels.contains(&0) {
                return;
            }
            let key = Key { pixels, pose: pose.quantized() };
            let image = cache.borrow_mut().0.get(window, key, || rasterize(pixels, pose));
            if let Some(image) = image {
                let bounds =
                    Bounds::new(bounds.origin, size(bounds.size.width, bounds.size.height));
                let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
            }
        },
    )
    .size_full()
}

/// Çizim, bu boyutta bir tasarım alanında yapılır; ölçek alana sığacak
/// biçimde seçilir. Domatesin gövdesi alanın yatay ortasında, `y = 12`'de.
pub const VIEW: [f32; 2] = [80.0, 92.0];
/// Tasarım alanının üst kenarı (buhar buraya kadar yükselir).
const VIEW_TOP: f32 = -40.0;
/// Gövdenin dikey yarıçapı; yatayda biraz daha geniştir.
const BODY: f32 = 34.0;

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let l = |x: f32, y: f32| x + (y - x) * t;
    gpui::Rgba { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }.into()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn oval(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<Path> {
    PathBuilder::from_oval(Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry)?)
}

fn polyline(points: &[(f32, f32)]) -> Option<Path> {
    let mut builder = PathBuilder::new();
    let (&(x, y), rest) = points.split_first()?;
    builder.move_to(x, y);
    for &(x, y) in rest {
        builder.line_to(x, y);
    }
    builder.finish()
}

/// Tasarım alanı koordinatlarında çizen küçük yardımcı.
struct Painter {
    pixmap: Pixmap,
    transform: Transform,
}

impl Painter {
    fn fill(&mut self, path: Option<Path>, color: Hsla, alpha: f32) {
        if let Some(path) = path
            && alpha > 0.0
        {
            let paint = paint(color, alpha);
            self.pixmap.fill_path(&path, &paint, FillRule::Winding, self.transform, None);
        }
    }

    fn stroke(&mut self, path: Option<Path>, color: Hsla, alpha: f32, width: f32) {
        if let Some(path) = path
            && alpha > 0.0
        {
            let stroke = Stroke {
                width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            self.pixmap.stroke_path(&path, &paint(color, alpha), &stroke, self.transform, None);
        }
    }
}

fn rasterize(pixels: [u32; 2], pose: Pose) -> Option<Arc<RenderImage>> {
    let [w, h] = pixels;
    let (wf, hf) = (w as f32, h as f32);
    let unit = (wf / VIEW[0]).min(hf / VIEW[1]);
    let transform = Transform::from_row(
        unit,
        0.0,
        0.0,
        unit,
        wf / 2.0,
        (hf - VIEW[1] * unit) / 2.0 - VIEW_TOP * unit,
    );
    let mut p = Painter { pixmap: Pixmap::new(w, h)?, transform };
    draw(&mut p, pose);
    to_image(p.pixmap)
}

fn draw(p: &mut Painter, pose: Pose) {
    let (an, t) = (pose.anger.clamp(0.0, 1.0), pose.time);
    let calm = 1.0 - an;
    let face = theme::surface();
    let light = theme::text();
    // Sakinken hafifçe aşağı yukarı sallanır.
    let cy = 12.0 + (t * 1.6).sin() * 1.2 * calm;

    // Gölge ve gövde.
    p.fill(oval(0.0, 47.0, 26.0, 3.0), gpui::black(), 0.25);
    let body = mix(theme::tomato(), theme::tomato_angry(), an);
    p.fill(oval(0.0, cy, BODY + 3.0, BODY), body, 1.0);
    let shine = oval(0.0, 0.0, 8.0, 4.5)
        .and_then(|o| o.transform(Transform::from_rotate(-34.0).post_translate(-16.0, cy - 15.0)));
    p.fill(shine, light, 0.35);

    // Yapraklar ve sap: sakinken rüzgârda sallanır, sinirlenince dikleşip titrer.
    let sway = (t * 1.8).sin() * 0.16 * calm + (t * 45.0).sin() * 0.04 * an;
    let crown = Transform::from_rotate(sway.to_degrees()).post_translate(0.0, cy - BODY + 3.0);
    for i in 0..5 {
        let a = i as f32 * 2.0 * PI / 5.0 + 0.3;
        let len = lerp(8.0, 10.0, an);
        let place = Transform::from_rotate((a * 0.25).to_degrees())
            .post_translate(a.cos() * len, a.sin() * 3.0 - an * 2.0)
            .post_concat(crown);
        p.fill(oval(0.0, 0.0, len, 2.8).and_then(|o| o.transform(place)), theme::leaf(), 1.0);
    }
    p.stroke(
        polyline(&[(0.0, 0.0), (1.5, -8.0)]).and_then(|s| s.transform(crown)),
        theme::leaf(),
        1.0,
        3.0,
    );

    // Gözler.
    let open = pose.open.clamp(0.12, 1.0);
    for side in [-1.0, 1.0] {
        let x = side * 12.0 + pose.look[0] * 3.0;
        let y = cy + 3.0 + pose.look[1] * 2.5;
        p.fill(oval(x, y, 3.8, 3.8 * open), face, 1.0);
        if pose.open > 0.5 {
            p.fill(PathBuilder::from_circle(x - 1.3, y - 1.5, 1.2), light, 1.0);
        }
        // Yanaklar sakinken pembe; kaşlar sinirlenince çatılır.
        p.fill(oval(side * 22.0, cy + 11.0, 6.0, 3.4), theme::blush(), 0.6 * calm);
        let brow = [(side * 18.0, cy - 8.0), (side * 7.0, cy - 5.0 + 3.0 * an)];
        p.stroke(polyline(&brow), face, an, 2.6);
    }

    // Ağız: gülümseme sinirlendikçe sıkılmış dişlere döner.
    let smile = ((0.45 - an) / 0.15).clamp(0.0, 1.0);
    let mut mouth = PathBuilder::new();
    mouth.move_to(-6.0, cy + 12.0);
    mouth.quad_to(0.0, cy + 17.0, 6.0, cy + 12.0);
    p.stroke(mouth.finish(), face, smile, 2.2);
    let teeth = ((an - 0.35) / 0.15).clamp(0.0, 1.0);
    if teeth > 0.0 {
        p.fill(rounded_rect(-8.0, cy + 11.0, 16.0, 6.0, 2.0), light, teeth);
        p.stroke(polyline(&[(-8.0, cy + 14.0), (8.0, cy + 14.0)]), face, teeth, 1.0);
        for x in [-3.0, 2.0] {
            p.stroke(polyline(&[(x, cy + 11.0), (x, cy + 17.0)]), face, teeth, 1.0);
        }
    }

    // Buhar: tepenin iki yanından yükselip dağılan bulutçuklar.
    if an > 0.01 {
        for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
            for i in 0..2 {
                let u = (t * 0.9 + i as f32 * 0.5 + k as f32 * 0.25).fract();
                let (x, y) = (side * (15.0 + u * 8.0), cy - BODY + 2.0 - u * 12.0);
                let puff = PathBuilder::from_circle(x, y, 2.5 + u * 3.5);
                p.fill(puff, light, an * (1.0 - u) * 0.8);
            }
        }
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.quad_to(x + w, y, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.quad_to(x + w, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.quad_to(x, y + h, x, y + h - r);
    b.line_to(x, y + r);
    b.quad_to(x, y, x + r, y);
    b.close();
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: Duration = Duration::from_millis(33);

    fn run(eye: &mut Eye, now: &mut Instant, mood: Mood, cursor: Option<[f32; 2]>, d: Duration) {
        let end = *now + d;
        while *now < end {
            *now += STEP;
            eye.update(*now, mood, cursor);
        }
    }

    fn eye_at(now: Instant) -> Eye {
        Eye::with_rng(now, fastrand::Rng::with_seed(7))
    }

    #[test]
    fn opens_from_closed() {
        let mut now = Instant::now();
        let mut eye = eye_at(now);
        assert_eq!(eye.open, 0.0);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(1));
        assert!(eye.open > 0.99, "{}", eye.open);
        run(&mut eye, &mut now, Mood::Relaxed, None, Duration::from_secs(1));
        assert!((eye.open - RELAXED_OPEN).abs() < 0.01, "{}", eye.open);
    }

    #[test]
    fn gets_angry_only_after_watching_for_a_while() {
        let mut now = Instant::now();
        let mut eye = eye_at(now);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(3));
        assert!(eye.anger < 0.01, "{}", eye.anger);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(6));
        assert!(eye.anger > 0.95, "{}", eye.anger);
        // Kullanıcı uzaklaşınca yatışır; dönünce saymaya baştan başlar.
        run(&mut eye, &mut now, Mood::Relaxed, None, Duration::from_secs(2));
        assert!(eye.anger < 0.01);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(3));
        assert!(eye.anger < 0.01);
    }

    #[test]
    fn follows_the_cursor_while_watching() {
        let mut now = Instant::now();
        let mut eye = eye_at(now);
        run(&mut eye, &mut now, Mood::Watching, Some([0.8, -0.3]), Duration::from_secs(1));
        assert!((eye.look[0] - 0.8).abs() < 0.01 && (eye.look[1] + 0.3).abs() < 0.01);
        // İmleç bilinmiyorsa kullanıcıya (düz ileri) bakar.
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(1));
        assert!(eye.look.iter().all(|v| v.abs() < 0.01));
    }

    #[test]
    fn looks_around_when_relaxed_and_stays_in_bounds() {
        let mut now = Instant::now();
        let mut eye = eye_at(now);
        let mut positions = Vec::new();
        for _ in 0..40 {
            run(&mut eye, &mut now, Mood::Relaxed, None, Duration::from_millis(500));
            let pose = eye.pose(now);
            assert!((0.0..=1.0).contains(&pose.open));
            assert!(pose.look.iter().all(|v| v.abs() <= 1.0));
            positions.push(pose.look);
        }
        let moved = positions.windows(2).filter(|p| p[0] != p[1]).count();
        assert!(moved > 10, "göz etrafa bakınmalı");
    }

    #[test]
    fn blinks_close_the_eye_briefly() {
        let mut now = Instant::now();
        let mut eye = eye_at(now);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(1));
        let blink_at = eye.next_blink;
        let wait = blink_at - now;
        run(&mut eye, &mut now, Mood::Watching, None, wait);
        let start = eye.blink_start.expect("göz kırpmalı");
        assert!(eye.pose(start + BLINK / 2).open < 0.05);
        assert!(eye.pose(start + BLINK).open > 0.99);
    }

    #[test]
    fn pose_time_advances_from_birth() {
        let now = Instant::now();
        let eye = eye_at(now);
        let later = eye.pose(now + Duration::from_millis(1500));
        assert!((later.time - 1.5).abs() < 1e-3);
        assert_ne!(eye.pose(now).quantized(), later.quantized());
    }

    #[test]
    fn cursor_direction_saturates_with_distance() {
        assert_eq!(cursor_direction(0.0, 0.0, 100.0), [0.0, 0.0]);
        let near = cursor_direction(10.0, 0.0, 100.0);
        let far = cursor_direction(10_000.0, 0.0, 100.0);
        assert!(near[0] < 0.1 && far[0] > 0.98 && far[1] == 0.0);
        let up = cursor_direction(0.0, -500.0, 100.0);
        assert!(up[1] < -0.8);
    }

    #[test]
    fn rasterizes_every_pose() {
        for open in [0.0, 0.3, 1.0] {
            for anger in [0.0, 0.4, 1.0] {
                for time in [0.0, 0.7, 123.4] {
                    let pose = Pose { open, look: [1.0, -1.0], anger, time };
                    assert!(rasterize([72, 79], pose).is_some());
                    assert!(rasterize([144, 158], pose).is_some());
                }
            }
        }
    }

    /// Domates, tasarım alanının dışına taşmamalı: kenar piksel sütunları ve
    /// satırları boş kalır.
    #[test]
    fn stays_inside_its_area() {
        let [w, h] = VIEW.map(|v| (v * 2.0) as u32);
        for (anger, time) in [(0.0, 0.0), (1.0, 0.3), (1.0, 0.9), (0.5, 2.2)] {
            let pose = Pose { open: 1.0, look: [1.0, 1.0], anger, time };
            let transform = Transform::from_row(2.0, 0.0, 0.0, 2.0, 80.0, -VIEW_TOP * 2.0);
            let mut p = Painter { pixmap: Pixmap::new(w, h).unwrap(), transform };
            draw(&mut p, pose);
            let alpha = |x: u32, y: u32| p.pixmap.pixel(x, y).unwrap().alpha();
            for x in 0..w {
                assert_eq!(alpha(x, 0), 0, "üst kenar, {anger} {time}");
                assert_eq!(alpha(x, h - 1), 0, "alt kenar");
            }
            for y in 0..h {
                assert_eq!(alpha(0, y), 0, "sol kenar");
                assert_eq!(alpha(w - 1, y), 0, "sağ kenar");
            }
        }
    }
}
