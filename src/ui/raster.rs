//! Halka ve domatesin ortak çizim yardımcıları: `tiny-skia` boyası,
//! pixmap → GPUI görüntüsü ve anahtar değişmedikçe yeniden çizmeyen önbellek.

use std::sync::Arc;

use gpui::{Hsla, RenderImage, Window, swap_rgba_pa_to_bgra};
use image::{Frame, RgbaImage};
use smallvec::smallvec;
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

/// Kenar yumuşatmalı düz boya; `alpha` rengin kendi saydamlığıyla çarpılır.
pub fn paint(color: Hsla, alpha: f32) -> Paint<'static> {
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

/// tiny-skia: önçarpımlı RGBA → GPUI: düz alfa BGRA.
pub fn to_image(pixmap: Pixmap) -> Option<Arc<RenderImage>> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut data = pixmap.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        swap_rgba_pa_to_bgra(pixel);
    }
    let buffer = RgbaImage::from_raw(w, h, data)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buffer)])))
}

/// Bir önceki görüntüyü saklar; anahtar değişmedikçe yeniden çizmez.
pub struct ImageCache<K> {
    key: Option<K>,
    image: Option<Arc<RenderImage>>,
}

impl<K> Default for ImageCache<K> {
    fn default() -> Self {
        Self { key: None, image: None }
    }
}

impl<K: PartialEq> ImageCache<K> {
    pub fn get(
        &mut self,
        window: &mut Window,
        key: K,
        render: impl FnOnce() -> Option<Arc<RenderImage>>,
    ) -> Option<Arc<RenderImage>> {
        if self.key.as_ref() != Some(&key) {
            let image = render()?;
            // Eski görüntüyü GPU atlasından çıkar, yoksa her karede birikir.
            if let Some(old) = self.image.replace(image) {
                let _ = window.drop_image(old);
            }
            self.key = Some(key);
        }
        self.image.clone()
    }
}

/// Tasarım alanı koordinatlarında çizen küçük yardımcı.
pub struct Painter {
    pub pixmap: Pixmap,
    pub transform: Transform,
}

impl Painter {
    pub fn fill(&mut self, path: Option<Path>, color: Hsla, alpha: f32) {
        if let Some(path) = path
            && alpha > 0.0
        {
            self.fill_with(&path, &paint(color, alpha));
        }
    }

    /// Hazır bir boyayla (ör. renk geçişi) doldurur.
    pub fn fill_with(&mut self, path: &Path, paint: &Paint) {
        self.pixmap.fill_path(path, paint, FillRule::Winding, self.transform, None);
    }

    pub fn stroke(&mut self, path: Option<Path>, color: Hsla, alpha: f32, width: f32) {
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

pub fn oval(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<Path> {
    PathBuilder::from_oval(Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry)?)
}

pub fn polyline(points: &[(f32, f32)]) -> Option<Path> {
    let mut builder = PathBuilder::new();
    let (&(x, y), rest) = points.split_first()?;
    builder.move_to(x, y);
    for &(x, y) in rest {
        builder.line_to(x, y);
    }
    builder.finish()
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
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
