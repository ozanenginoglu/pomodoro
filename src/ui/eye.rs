//! Mola bekçisi göz.
//!
//! Mola sürerken dairede bir göz belirir:
//! - Kullanıcı uzaktayken (mola işliyor) gevşektir: göz kapakları yarı
//!   kapalı, ara sıra etrafa bakınır.
//! - Kullanıcı bilgisayar başındayken (mola tutuluyor) iri açılır, imleci
//!   izler ve birkaç saniye sonra kaşlarını çatar.
//!
//! Hareket modeli (`Eye`) zamanı parametre olarak alır ve test edilir; çizim,
//! halka gibi `tiny-skia` ile gerçek piksel çözünürlüğünde yapılır.

use std::{cell::RefCell, f32::consts::PI, rc::Rc, sync::Arc, time::Duration, time::Instant};

use gpui::{
    Bounds, Corners, Hsla, IntoElement, RenderImage, Styled, Window, canvas, size,
    swap_rgba_pa_to_bgra,
};
use image::{Frame, RgbaImage};
use smallvec::smallvec;
use tiny_skia::{FillRule, LineCap, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// Gözün ruh hali.
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
    /// Göz kapaklarının açıklığı, 0 (kapalı) ..= 1.
    pub open: f32,
    /// Bakış yönü, her eksende -1 ..= 1 (y aşağı).
    pub look: [f32; 2],
    /// Kaş çatma, 0 ..= 1.
    pub frown: f32,
}

impl Pose {
    /// Gözle fark edilmeyecek kadar küçük değişiklikleri yok sayan karşılaştırma
    /// anahtarı: aynıysa yeniden çizmeye gerek yok.
    pub fn quantized(&self) -> [i32; 4] {
        let q = |v: f32| (v * 512.0).round() as i32;
        [q(self.open), q(self.look[0]), q(self.look[1]), q(self.frown)]
    }
}

const BLINK: Duration = Duration::from_millis(160);
/// Bu kadar süre bilgisayar başında kalınca kaşlar çatılmaya başlar…
const FROWN_AFTER: Duration = Duration::from_secs(4);
/// …ve bu sürede tamamen çatılır.
const FROWN_RAMP: Duration = Duration::from_secs(3);
const RELAXED_OPEN: f32 = 0.55;

pub struct Eye {
    open: f32,
    look: [f32; 2],
    frown: f32,
    target_look: [f32; 2],
    next_glance: Instant,
    blink_start: Option<Instant>,
    next_blink: Instant,
    watching_since: Option<Instant>,
    last: Instant,
    rng: fastrand::Rng,
}

impl Eye {
    /// Kapalı başlar; ilk güncellemelerle açılır.
    pub fn new(now: Instant) -> Self {
        Self::with_rng(now, fastrand::Rng::new())
    }

    fn with_rng(now: Instant, mut rng: fastrand::Rng) -> Self {
        let next_blink = now + random_between(&mut rng, 2.5, 6.0);
        Self {
            open: 0.0,
            look: [0.0; 2],
            frown: 0.0,
            target_look: [0.0; 2],
            next_glance: now + Duration::from_millis(800),
            blink_start: None,
            next_blink,
            watching_since: None,
            last: now,
            rng,
        }
    }

    /// Her karede çağrılır. `cursor`: imlecin gözden yönü (-1 ..= 1); yoksa
    /// göz kullanıcıya, yani düz ileri bakar.
    pub fn update(&mut self, now: Instant, mood: Mood, cursor: Option<[f32; 2]>) {
        // Uzun bir aradan (askı, gizli pencere) sonra tek adımda sıçramasın.
        let dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;

        let (open, frown, look, look_rate) = match mood {
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
                let watched = now.saturating_duration_since(since).saturating_sub(FROWN_AFTER);
                let frown = (watched.as_secs_f32() / FROWN_RAMP.as_secs_f32()).min(1.0);
                let look = cursor.map_or([0.0; 2], |c| c.map(|v| v.clamp(-1.0, 1.0)));
                (1.0, frown, look, 10.0)
            }
        };
        self.open = approach(self.open, open, 6.0, dt);
        self.frown = approach(self.frown, frown, 4.0, dt);
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
        Pose { open: self.open * blink, look: self.look, frown: self.frown }
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

/// İmlecin göze göre yönü: yakındayken küçük, uzaklaştıkça 1'e yaklaşır.
/// `dx`, `dy`: imlecin göz merkezine uzaklığı; `reach`: yarı yola gelinen mesafe.
pub fn cursor_direction(dx: f32, dy: f32, reach: f32) -> [f32; 2] {
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < f32::EPSILON {
        return [0.0; 2];
    }
    let strength = distance / (distance + reach.max(1.0));
    [dx / distance * strength, dy / distance * strength]
}

// ---- Çizim -------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeStyle {
    pub sclera: Hsla,
    pub iris: Hsla,
    pub pupil: Hsla,
    pub line: Hsla,
}

#[derive(Default)]
pub struct EyeCache {
    key: Option<Key>,
    image: Option<Arc<RenderImage>>,
}

pub type SharedEyeCache = Rc<RefCell<EyeCache>>;

#[derive(Clone, Copy, PartialEq)]
struct Key {
    pixels: [u32; 2],
    pose: [i32; 4],
    style: [u32; 4],
}

/// Gözü bulunduğu alana çizer.
pub fn eye(cache: SharedEyeCache, pose: Pose, style: EyeStyle) -> impl IntoElement {
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
            let image = cache.borrow_mut().get(window, pixels, pose, style);
            if let Some(image) = image {
                let bounds =
                    Bounds::new(bounds.origin, size(bounds.size.width, bounds.size.height));
                let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
            }
        },
    )
    .size_full()
}

impl EyeCache {
    fn get(
        &mut self,
        window: &mut Window,
        pixels: [u32; 2],
        pose: Pose,
        style: EyeStyle,
    ) -> Option<Arc<RenderImage>> {
        let key = Key {
            pixels,
            pose: pose.quantized(),
            style: [style.sclera, style.iris, style.pupil, style.line].map(pack),
        };
        if self.key != Some(key) {
            let image = rasterize(pixels, window.scale_factor(), pose, style)?;
            if let Some(old) = self.image.replace(image) {
                let _ = window.drop_image(old);
            }
            self.key = Some(key);
        }
        self.image.clone()
    }
}

fn pack(c: Hsla) -> u32 {
    let c = c.to_rgb();
    u32::from_be_bytes([c.r, c.g, c.b, c.a].map(|v| (v * 255.0).round() as u8))
}

fn paint(color: Hsla, alpha: f32) -> Paint<'static> {
    let c = color.to_rgb();
    let mut paint = Paint::default();
    paint.set_color_rgba8(
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
        (c.a * alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    paint.anti_alias = true;
    paint
}

/// Göz kapaklarının biçimi. Açıkken badem; kapanırken iki kapak aşağı
/// kıvrık tek bir çizgide ("‿") buluşur.
struct Lids {
    upper: Path,
    lower: Path,
    almond: Path,
}

fn lids(cx: f32, cy: f32, a: f32, b: f32, open: f32, squint: f32) -> Option<Lids> {
    // Kübik eğride iki kontrol noktası aynı yükseklikteyken tepe, kontrol
    // yüksekliğinin 3/4'üne çıkar.
    let closed = 0.35 * b * (1.0 - open);
    let upper_peak = -b * open * (1.0 - 0.45 * squint) + closed;
    let lower_peak = 0.7 * b * open + closed;
    let (left, right) = (cx - a, cx + a);
    let k = a * 0.45;
    let curve = |builder: &mut PathBuilder, from: f32, to: f32, peak: f32| {
        let y = cy + peak / 0.75;
        let dir = (to - from).signum();
        builder.cubic_to(from + dir * (a - k), y, to - dir * (a - k), y, to, cy);
    };

    let mut upper = PathBuilder::new();
    upper.move_to(left, cy);
    curve(&mut upper, left, right, upper_peak);
    let mut lower = PathBuilder::new();
    lower.move_to(right, cy);
    curve(&mut lower, right, left, lower_peak);
    let mut almond = PathBuilder::new();
    almond.move_to(left, cy);
    curve(&mut almond, left, right, upper_peak);
    curve(&mut almond, right, left, lower_peak);
    almond.close();
    Some(Lids { upper: upper.finish()?, lower: lower.finish()?, almond: almond.finish()? })
}

fn rasterize(
    pixels: [u32; 2],
    scale: f32,
    pose: Pose,
    style: EyeStyle,
) -> Option<Arc<RenderImage>> {
    let [w, h] = pixels;
    let mut pixmap = Pixmap::new(w, h)?;
    let (wf, hf) = (w as f32, h as f32);
    let identity = Transform::identity();
    let line = 1.8 * scale;

    // Göz, alanın alt kısmında; üstte kaşa yer kalır.
    let a = wf * 0.42;
    let b = (hf * 0.3).min(a * 0.62);
    let (cx, cy) = (wf / 2.0, hf * 0.62);
    let lids = lids(cx, cy, a, b, pose.open, pose.frown)?;

    if pose.open > 0.02 {
        let mut mask = Mask::new(w, h)?;
        mask.fill_path(&lids.almond, FillRule::Winding, true, identity);
        pixmap.fill_path(
            &lids.almond,
            &paint(style.sclera, 1.0),
            FillRule::Winding,
            identity,
            None,
        );

        let iris_r = b * 0.62;
        let ix = cx + pose.look[0] * (a - iris_r) * 0.8;
        let iy = cy + pose.look[1] * b * 0.45;
        let circle = |x: f32, y: f32, r: f32| PathBuilder::from_circle(x, y, r);
        let fill = |pixmap: &mut Pixmap, path: Option<Path>, paint: Paint| {
            if let Some(path) = path {
                pixmap.fill_path(&path, &paint, FillRule::Winding, identity, Some(&mask));
            }
        };
        fill(&mut pixmap, circle(ix, iy, iris_r), paint(style.iris, 1.0));
        fill(&mut pixmap, circle(ix, iy, iris_r * 0.5), paint(style.pupil, 1.0));
        fill(
            &mut pixmap,
            circle(ix - iris_r * 0.3, iy - iris_r * 0.32, iris_r * 0.17),
            paint(style.sclera, 0.9),
        );
    }

    let stroke = Stroke { width: line, line_cap: LineCap::Round, ..Stroke::default() };
    pixmap.stroke_path(&lids.upper, &paint(style.line, 1.0), &stroke, identity, None);
    pixmap.stroke_path(&lids.lower, &paint(style.line, 0.55), &stroke, identity, None);

    // Çatık kaş: ortada aşağı inen bir "V".
    if pose.frown > 0.02 {
        // Kaş, kısılan göz kapağıyla birlikte aşağı iner.
        let top = cy - b * (1.0 - 0.35 * pose.frown) - hf * 0.1;
        let dip = pose.frown * b * 0.6;
        let mut brow = PathBuilder::new();
        brow.move_to(cx - a * 0.8, top - dip * 0.3);
        brow.line_to(cx, top + dip);
        brow.line_to(cx + a * 0.8, top - dip * 0.3);
        if let Some(brow) = brow.finish() {
            let stroke = Stroke { width: line * 2.0, line_cap: LineCap::Round, ..stroke.clone() };
            pixmap.stroke_path(&brow, &paint(style.line, pose.frown), &stroke, identity, None);
        }
    }

    let mut data = pixmap.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        swap_rgba_pa_to_bgra(pixel);
    }
    let buffer = RgbaImage::from_raw(w, h, data)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buffer)])))
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
    fn frowns_only_after_watching_for_a_while() {
        let mut now = Instant::now();
        let mut eye = eye_at(now);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(3));
        assert!(eye.frown < 0.01, "{}", eye.frown);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(6));
        assert!(eye.frown > 0.95, "{}", eye.frown);
        // Kullanıcı uzaklaşınca yatışır; dönünce saymaya baştan başlar.
        run(&mut eye, &mut now, Mood::Relaxed, None, Duration::from_secs(2));
        assert!(eye.frown < 0.01);
        run(&mut eye, &mut now, Mood::Watching, None, Duration::from_secs(3));
        assert!(eye.frown < 0.01);
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
        let style = EyeStyle {
            sclera: gpui::white(),
            iris: gpui::green(),
            pupil: gpui::black(),
            line: gpui::white(),
        };
        for open in [0.0, 0.3, 1.0] {
            for frown in [0.0, 1.0] {
                let pose = Pose { open, look: [1.0, -1.0], frown };
                assert!(rasterize([96, 64], 1.0, pose, style).is_some());
            }
        }
    }
}
