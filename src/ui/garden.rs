//! Domates bahçesi: sayaç durmuşken dairede bir saksı belirir. Bugün biten
//! her çalışma turu bitkiye bir domates ekler, atlanan turlar ham (yeşil)
//! kalır. Bitki de turlarla büyür: tur yokken filiz, akşama dallı bir bitki.
//! En son biten domatesin yüzü vardır.
//!
//! Çizim, domates ve halka gibi `tiny-skia` ile gerçek piksel çözünürlüğünde
//! yapılır ve turlar değişmedikçe yeniden çizilmez.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{Bounds, Corners, Hsla, IntoElement, RenderImage, Styled, canvas};
use tiny_skia::{
    Color, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pixmap, Point, SpreadMode,
    Transform,
};

use super::{
    raster::{ImageCache, Painter, oval, rounded_rect, to_image},
    theme,
};
use crate::history::Outcome;

/// Tasarım alanı: `x ∈ [68, 132]`, `y ∈ [38, 148]` (saksının ortası `x = 100`).
pub const VIEW: [f32; 2] = [64.0, 110.0];
const ORIGIN: [f32; 2] = [68.0, 38.0];

/// Bitkide yer alan en fazla domates; fazlası yalnızca sayı olarak gösterilir.
pub const MAX_TOMATOES: usize = 12;

/// Domateslerin yerleri, eklenme sırasıyla (alttan yukarı, iki yana).
const SPOTS: [(f32, f32); MAX_TOMATOES] = [
    (86.0, 88.0),
    (114.0, 81.0),
    (93.0, 70.0),
    (108.0, 64.0),
    (80.0, 70.0),
    (120.0, 62.0),
    (100.0, 54.0),
    (88.0, 56.0),
    (113.0, 48.0),
    (124.0, 76.0),
    (76.0, 82.0),
    (100.0, 74.0),
];

/// Bitkinin büyüklüğü: 0 filiz, 3 tam bitki.
fn stage(rounds: usize) -> usize {
    match rounds {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        _ => 3,
    }
}

#[derive(Default)]
pub struct GardenCache(ImageCache<Key>);

pub type SharedGardenCache = Rc<RefCell<GardenCache>>;

#[derive(Clone, PartialEq)]
struct Key {
    pixels: [u32; 2],
    rounds: Vec<Outcome>,
}

/// Bahçeyi bulunduğu alana çizer.
pub fn garden(cache: SharedGardenCache, rounds: &[Outcome]) -> impl IntoElement {
    let rounds = rounds.to_vec();
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
            let key = Key { pixels, rounds: rounds.clone() };
            let image = cache.borrow_mut().0.get(window, key, || rasterize(pixels, &rounds));
            if let Some(image) = image {
                let bounds = Bounds::new(bounds.origin, bounds.size);
                let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
            }
        },
    )
    .size_full()
}

fn transform_for([w, h]: [f32; 2]) -> Transform {
    let unit = (w / VIEW[0]).min(h / VIEW[1]);
    Transform::from_row(
        unit,
        0.0,
        0.0,
        unit,
        (w - VIEW[0] * unit) / 2.0 - ORIGIN[0] * unit,
        (h - VIEW[1] * unit) / 2.0 - ORIGIN[1] * unit,
    )
}

fn rasterize(pixels: [u32; 2], rounds: &[Outcome]) -> Option<Arc<RenderImage>> {
    let [w, h] = pixels;
    let transform = transform_for([w as f32, h as f32]);
    let mut p = Painter { pixmap: Pixmap::new(w, h)?, transform };
    draw(&mut p, rounds);
    to_image(p.pixmap)
}

fn draw(p: &mut Painter, rounds: &[Outcome]) {
    pot(p);
    plant(p, rounds);
}

fn skia_color(color: Hsla) -> Color {
    let c = color.to_rgb();
    Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(Color::BLACK)
}

/// Kübik eğri: `from` → `to`, iki kontrol noktasıyla.
fn curve(from: (f32, f32), c1: (f32, f32), c2: (f32, f32), to: (f32, f32)) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(from.0, from.1);
    b.cubic_to(c1.0, c1.1, c2.0, c2.1, to.0, to.1);
    b.finish()
}

/// Saksı: geniş ağız, içe kavisli gövde, yuvarlatılmış dip, altında tabak ve gölge.
fn pot(p: &mut Painter) {
    p.fill(oval(100.0, 143.5, 27.0, 3.0), gpui::black(), 0.28);

    // Tabak.
    let mut saucer = PathBuilder::new();
    saucer.move_to(76.0, 138.5);
    saucer.line_to(124.0, 138.5);
    saucer.quad_to(123.0, 142.5, 119.0, 142.5);
    saucer.line_to(81.0, 142.5);
    saucer.quad_to(77.0, 142.5, 76.0, 138.5);
    saucer.close();
    p.fill(saucer.finish(), theme::pot_deep(), 1.0);
    p.fill(rounded_rect(75.0, 137.0, 50.0, 2.6, 1.3), theme::pot_dark(), 1.0);

    // Gövde: soldan sağa ışık geçişi.
    let mut body = PathBuilder::new();
    body.move_to(81.0, 117.0);
    body.line_to(119.0, 117.0);
    body.quad_to(117.5, 128.0, 115.5, 134.5);
    body.quad_to(114.8, 137.2, 112.0, 137.2);
    body.line_to(88.0, 137.2);
    body.quad_to(85.2, 137.2, 84.5, 134.5);
    body.quad_to(82.5, 128.0, 81.0, 117.0);
    body.close();
    if let Some(body) = body.finish() {
        let stops = vec![
            GradientStop::new(0.0, skia_color(theme::pot_dark())),
            GradientStop::new(0.35, skia_color(theme::pot())),
            GradientStop::new(0.7, skia_color(theme::pot_light())),
            GradientStop::new(1.0, skia_color(theme::pot_dark())),
        ];
        let shader = LinearGradient::new(
            Point::from_xy(81.0, 0.0),
            Point::from_xy(119.0, 0.0),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        );
        if let Some(shader) = shader {
            let paint = Paint { shader, anti_alias: true, ..Paint::default() };
            p.fill_with(&body, &paint);
        }
    }

    // Ağız, altındaki gölge, parlaklık ve toprak.
    p.fill(rounded_rect(77.5, 110.5, 45.0, 7.5, 2.5), theme::pot(), 1.0);
    p.fill(rounded_rect(77.5, 115.6, 45.0, 2.4, 1.2), theme::pot_dark(), 0.7);
    p.fill(rounded_rect(80.0, 111.6, 18.0, 1.6, 0.8), theme::text(), 0.22);
    p.fill(oval(100.0, 111.2, 20.0, 2.2), theme::soil(), 1.0);
}

fn leaf(p: &mut Painter, x: f32, y: f32, len: f32, angle: f32, color: Hsla) {
    let mut b = PathBuilder::new();
    b.move_to(0.0, 0.0);
    b.quad_to(len * 0.5, -len * 0.45, len, 0.0);
    b.quad_to(len * 0.5, len * 0.45, 0.0, 0.0);
    b.close();
    let place = Transform::from_rotate(angle).post_translate(x, y);
    p.fill(b.finish().and_then(|path| path.transform(place)), color, 1.0);
}

fn plant(p: &mut Painter, rounds: &[Outcome]) {
    let stage = stage(rounds.len());
    let top = [98.0, 80.0, 62.0, 46.0][stage];
    let mid = (111.0 + top) / 2.0;
    let stem = theme::stem();
    let main_width = if stage == 0 { 1.8 } else { 2.4 };
    p.stroke(
        curve((100.0, 111.0), (98.0, mid + 6.0), (102.0, mid - 6.0), (100.0, top)),
        stem,
        1.0,
        main_width,
    );
    if stage == 0 {
        leaf(p, 100.0, 99.0, 8.0, -150.0, theme::leaf());
        leaf(p, 100.0, 99.0, 8.0, -30.0, theme::leaf());
        return;
    }
    leaf(p, 100.0, 104.0, 8.0, -165.0, theme::leaf_dark());
    leaf(p, 100.0, 102.0, 8.0, -15.0, theme::leaf_dark());
    let branch = |p: &mut Painter, pts: [(f32, f32); 4], width: f32| {
        p.stroke(curve(pts[0], pts[1], pts[2], pts[3]), stem, 1.0, width);
    };
    branch(p, [(100.0, 96.0), (94.0, 92.0), (89.0, 89.0), (86.0, 84.0)], 1.8);
    branch(p, [(100.0, 90.0), (106.0, 86.0), (111.0, 82.0), (114.0, 77.0)], 1.8);
    leaf(p, 100.0, top + 2.0, 7.0, -140.0, theme::leaf());
    leaf(p, 100.0, top + 2.0, 7.0, -40.0, theme::leaf());
    if stage >= 2 {
        branch(p, [(100.0, 78.0), (97.0, 74.0), (95.0, 70.0), (93.0, 66.0)], 1.6);
        branch(p, [(100.0, 72.0), (104.0, 68.0), (106.0, 64.0), (108.0, 60.0)], 1.6);
        branch(p, [(99.0, 86.0), (92.0, 80.0), (85.0, 72.0), (80.0, 66.0)], 1.6);
        branch(p, [(101.0, 80.0), (110.0, 72.0), (116.0, 64.0), (120.0, 58.0)], 1.6);
        leaf(p, 92.0, 88.0, 7.0, -170.0, theme::leaf());
        leaf(p, 108.0, 84.0, 7.0, -10.0, theme::leaf());
        leaf(p, 84.0, 70.0, 6.0, -120.0, theme::leaf_dark());
        leaf(p, 116.0, 64.0, 6.0, -60.0, theme::leaf_dark());
    }
    if stage >= 3 {
        branch(p, [(100.0, 60.0), (96.0, 57.0), (92.0, 55.0), (88.0, 52.0)], 1.4);
        branch(p, [(100.0, 56.0), (105.0, 52.0), (110.0, 47.0), (113.0, 44.0)], 1.4);
        branch(p, [(104.0, 86.0), (114.0, 82.0), (120.0, 78.0), (124.0, 72.0)], 1.4);
        branch(p, [(96.0, 92.0), (88.0, 88.0), (81.0, 84.0), (76.0, 78.0)], 1.4);
        leaf(p, 94.0, 58.0, 6.0, -150.0, theme::leaf());
        leaf(p, 108.0, 52.0, 6.0, -30.0, theme::leaf());
        leaf(p, 120.0, 76.0, 6.0, 10.0, theme::leaf_dark());
        leaf(p, 80.0, 82.0, 6.0, 170.0, theme::leaf_dark());
    }

    let shown = &rounds[..rounds.len().min(MAX_TOMATOES)];
    let last_done = shown.iter().rposition(|&o| o == Outcome::Done);
    for (i, (&outcome, &(x, y))) in shown.iter().zip(&SPOTS).enumerate() {
        let ripe = outcome == Outcome::Done;
        tomato(p, x, y, if ripe { 6.2 } else { 4.6 }, ripe, Some(i) == last_done);
    }
}

/// Küçük domates: gövde, parlaklık, yaprak tacı; istenirse gülen bir yüz.
fn tomato(p: &mut Painter, x: f32, y: f32, r: f32, ripe: bool, face: bool) {
    let (body, shade) = if ripe {
        (theme::tomato(), theme::tomato_shade())
    } else {
        (theme::unripe(), theme::unripe_shade())
    };
    p.fill(oval(x, y + r * 0.1, r, r * 0.9), shade, 1.0);
    p.fill(oval(x, y, r, r * 0.86), body, 1.0);
    let shine = oval(0.0, 0.0, r * 0.26, r * 0.15).and_then(|o| {
        o.transform(Transform::from_rotate(-30.0).post_translate(x - r * 0.38, y - r * 0.34))
    });
    p.fill(shine, theme::text(), 0.45);

    let ly = y - r * 0.78;
    for i in 0..5 {
        let shift = match i {
            0 | 1 => -r * 0.28,
            3 | 4 => r * 0.28,
            _ => 0.0,
        };
        let place = Transform::from_rotate((i as f32 - 2.0) * 38.0).post_translate(x, ly);
        let sepal = oval(shift, 0.0, r * 0.42, r * 0.15).and_then(|o| o.transform(place));
        p.fill(sepal, theme::leaf(), 1.0);
    }
    p.fill(
        rounded_rect(x - r * 0.09, ly - r * 0.42, r * 0.18, r * 0.45, r * 0.09),
        theme::leaf_dark(),
        1.0,
    );

    if face {
        let ink = theme::surface();
        for side in [-1.0, 1.0] {
            p.fill(PathBuilder::from_circle(x + side * r * 0.3, y + r * 0.08, r * 0.1), ink, 1.0);
            p.fill(oval(x + side * r * 0.54, y + r * 0.3, r * 0.14, r * 0.08), theme::blush(), 0.6);
        }
        let mut mouth = PathBuilder::new();
        mouth.move_to(x - r * 0.16, y + r * 0.32);
        mouth.quad_to(x, y + r * 0.48, x + r * 0.16, y + r * 0.32);
        p.stroke(mouth.finish(), ink, 1.0, r * 0.08);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(done: usize, skipped_at: &[usize]) -> Vec<Outcome> {
        (0..done)
            .map(|i| if skipped_at.contains(&i) { Outcome::Skipped } else { Outcome::Done })
            .collect()
    }

    #[test]
    fn plant_grows_with_rounds() {
        assert_eq!([0, 1, 2, 3, 5, 6, 20].map(stage), [0, 1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn rasterizes_every_stage() {
        for n in [0, 1, 3, 6, 12, 30] {
            let rounds = day(n, &[2]);
            assert!(rasterize([61, 105], &rounds).is_some(), "{n}");
            assert!(rasterize([122, 209], &rounds).is_some(), "{n}");
        }
    }

    /// Bahçe tasarım alanının dışına taşmamalı: kenar piksel sütunları ve
    /// satırları boş kalır.
    #[test]
    fn stays_inside_its_area() {
        let [w, h] = VIEW.map(|v| (v * 3.0) as u32);
        for n in [0, 2, 5, 12] {
            let transform = transform_for([w as f32, h as f32]);
            let mut p = Painter { pixmap: Pixmap::new(w, h).unwrap(), transform };
            draw(&mut p, &day(n, &[1]));
            let alpha = |x: u32, y: u32| p.pixmap.pixel(x, y).unwrap().alpha();
            for x in 0..w {
                assert_eq!(alpha(x, 0), 0, "üst kenar, {n} tur");
                assert_eq!(alpha(x, h - 1), 0, "alt kenar, {n} tur");
            }
            for y in 0..h {
                assert_eq!(alpha(0, y), 0, "sol kenar, {n} tur");
                assert_eq!(alpha(w - 1, y), 0, "sağ kenar, {n} tur");
            }
        }
    }

    #[test]
    fn more_rounds_paint_more_tomato() {
        // Kırmızıya yakın piksel sayısı turlarla artar; atlanan tur kırmızı eklemez.
        let red = |rounds: &[Outcome]| {
            let transform = transform_for([128.0, 220.0]);
            let mut p = Painter { pixmap: Pixmap::new(128, 220).unwrap(), transform };
            draw(&mut p, rounds);
            p.pixmap.pixels().iter().filter(|c| c.red() > 200 && c.green() < 120).count()
        };
        let one = red(&day(1, &[]));
        let three = red(&day(3, &[]));
        assert!(red(&[]) < one && one < three, "{one} {three}");
        assert!(red(&day(3, &[2])) < three, "atlanan tur ham kalır");
    }
}
