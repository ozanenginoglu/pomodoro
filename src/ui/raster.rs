//! Halka ve domatesin ortak çizim yardımcıları: `tiny-skia` boyası,
//! pixmap → GPUI görüntüsü ve anahtar değişmedikçe yeniden çizmeyen önbellek.

use std::sync::Arc;

use gpui::{Hsla, RenderImage, Window, swap_rgba_pa_to_bgra};
use image::{Frame, RgbaImage};
use smallvec::smallvec;
use tiny_skia::{Paint, Pixmap};

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
