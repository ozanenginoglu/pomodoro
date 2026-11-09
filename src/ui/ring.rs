//! Daire ve ilerleme halkası.
//!
//! GPUI'nin vektör yolu çizicisi kenarları yumuşatmadığı için daireyi, izi ve
//! yayı `tiny-skia` ile ekranın gerçek piksel çözünürlüğünde (HiDPI dahil)
//! kenar yumuşatmalı olarak çizip görüntü olarak basıyoruz. Görüntü yalnızca
//! boyut, renk ya da halka oranı değiştiğinde yeniden üretilir (en fazla
//! saniyede bir); hover animasyonu gibi diğer yeniden çizimler önbellekten gelir.

use std::{cell::RefCell, f32::consts::TAU, rc::Rc, sync::Arc};

use gpui::{Bounds, Corners, Hsla, IntoElement, RenderImage, Styled, canvas, size};
use tiny_skia::{FillRule, LineCap, PathBuilder, Pixmap, Stroke, Transform};

use super::raster::{ImageCache, paint, to_image};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingStyle {
    /// Halka kalınlığı (mantıksal piksel).
    pub width: f32,
    pub surface: Hsla,
    pub track: Hsla,
    pub color: Hsla,
}

/// Bir önceki halkayı saklar; anahtar değişmedikçe yeniden çizmez.
#[derive(Default)]
pub struct RingCache(ImageCache<Key>);

#[derive(Clone, Copy, PartialEq)]
struct Key {
    pixels: u32,
    /// Oran 1/4096 hassasiyetle; 200 px'lik dairede bir pikselden çok daha ince.
    fraction: u32,
    style: [u32; 4],
}

pub type SharedRingCache = Rc<RefCell<RingCache>>;

/// `fraction` (0..=1) kadar dolu, saat 12'den saat yönünde ilerleyen halka
/// ve altındaki dolu daire. Bulunduğu alanı kaplar.
pub fn ring(cache: SharedRingCache, fraction: f32, style: RingStyle) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let scale = window.scale_factor();
            // Pencere kare değilse (ör. döşemeli bir pencere yöneticisi büyüttüyse)
            // daire, ortadaki karenin içine çizilir; elipse dönüşmez.
            let side = bounds.size.width.min(bounds.size.height);
            let bounds = Bounds::centered_at(bounds.center(), size(side, side));
            let pixels = (f32::from(side) * scale).round() as u32;
            if pixels == 0 {
                return;
            }
            let fraction = fraction.clamp(0.0, 1.0);
            let key = Key {
                pixels,
                fraction: (fraction * 4096.0).round() as u32,
                style: style_key(style),
            };
            let image =
                cache.borrow_mut().0.get(window, key, || rasterize(pixels, scale, fraction, style));
            if let Some(image) = image {
                let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
            }
        },
    )
    .size_full()
}

fn style_key(style: RingStyle) -> [u32; 4] {
    let pack = |c: Hsla| {
        let c = c.to_rgb();
        u32::from_be_bytes([c.r, c.g, c.b, c.a].map(|v| (v * 255.0).round() as u8))
    };
    [style.width.to_bits(), pack(style.surface), pack(style.track), pack(style.color)]
}

fn rasterize(pixels: u32, scale: f32, fraction: f32, style: RingStyle) -> Option<Arc<RenderImage>> {
    let mut pixmap = Pixmap::new(pixels, pixels)?;
    let size = pixels as f32;
    let center = size / 2.0;
    let width = style.width * scale;
    // Dış kenar pikselin yarısı kadar içeride: kenar yumuşatması kesilmesin.
    let outer = center - 0.5;
    let ring_radius = outer - width / 2.0;
    let identity = Transform::identity();

    let disc = PathBuilder::from_circle(center, center, outer)?;
    pixmap.fill_path(&disc, &paint(style.surface, 1.0), FillRule::Winding, identity, None);

    let track = PathBuilder::from_circle(center, center, ring_radius)?;
    let stroke = Stroke { width, ..Stroke::default() };
    pixmap.stroke_path(&track, &paint(style.track, 1.0), &stroke, identity, None);

    if fraction > 0.0 {
        let full = fraction >= 0.9995;
        let stroke = Stroke {
            width,
            line_cap: if full { LineCap::Butt } else { LineCap::Round },
            ..Stroke::default()
        };
        let arc = if full { Some(track) } else { arc_path(center, center, ring_radius, fraction) };
        if let Some(arc) = arc {
            pixmap.stroke_path(&arc, &paint(style.color, 1.0), &stroke, identity, None);
        }
    }

    to_image(pixmap)
}

/// Saat 12'den saat yönünde `fraction` turluk yay; ≤90°'lik kübik Bézier
/// parçalarıyla (hata < 0.03 px).
fn arc_path(cx: f32, cy: f32, r: f32, fraction: f32) -> Option<tiny_skia::Path> {
    // Açı saat 12'den saat yönünde; ekran y ekseni aşağı baktığı için y = cy - r·cos(θ).
    let at = |theta: f32| (cx + r * theta.sin(), cy - r * theta.cos());
    let total = fraction * TAU;
    let segments = (total / (TAU / 4.0)).ceil().max(1.0) as usize;
    let step = total / segments as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan() * r;

    let mut builder = PathBuilder::new();
    let (x0, y0) = at(0.0);
    builder.move_to(x0, y0);
    for i in 0..segments {
        let (a0, a1) = (i as f32 * step, (i + 1) as f32 * step);
        let (p0, p1) = (at(a0), at(a1));
        // Teğet yönü (saat yönünde): (cos θ, sin θ).
        let c0 = (p0.0 + k * a0.cos(), p0.1 + k * a0.sin());
        let c1 = (p1.0 - k * a1.cos(), p1.1 - k * a1.sin());
        builder.cubic_to(c0.0, c0.1, c1.0, c1.1, p1.0, p1.1);
    }
    builder.finish()
}
