//! Kamera karelerini yüz algılama modelinin girdisine çevirir.
//!
//! Kameralar farklı piksel biçimleri verir (YUYV, NV12, I420, MJPEG …). Tüm
//! kareyi RGB'ye çevirmek yerine yalnızca modelin 320×240'lık girdisine düşen
//! pikseller okunur (en yakın komşu); böylece 1080p bir kare de ucuzdur.

/// Modelin girdi boyutu.
pub const INPUT_WIDTH: usize = 320;
pub const INPUT_HEIGHT: usize = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// Paketli 4:2:2: Y0 U Y1 V.
    Yuyv,
    /// Y düzlemi + iç içe UV düzlemi (4:2:0).
    Nv12,
    /// Y, U ve V düzlemleri (4:2:0; YU12).
    I420,
    Mjpeg,
    Gray,
    Rgb,
    /// Yalnızca Windows kameraları verir.
    #[cfg_attr(not(windows), allow(dead_code))]
    Bgr,
}

/// Kameradan gelen bir kare.
pub struct Frame<'a> {
    pub format: PixelFormat,
    pub width: usize,
    pub height: usize,
    /// Bir satırın bayt sayısı (paketli biçimlerde) ya da Y düzleminin satır
    /// uzunluğu (düzlemli biçimlerde). Sıfırsa sıkışık kabul edilir.
    pub stride: usize,
    pub data: &'a [u8],
}

/// Kareyi modelin girdisine çevirir: 1×3×240×320, kanal sırası RGB, değerler
/// `(v - 127) / 128`. Kare eksik ya da bozuksa `None`.
pub fn to_input(frame: &Frame) -> Option<Vec<f32>> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    if frame.format == PixelFormat::Mjpeg {
        let image = image::load_from_memory_with_format(frame.data, image::ImageFormat::Jpeg)
            .ok()?
            .to_rgb8();
        let (width, height) = (image.width() as usize, image.height() as usize);
        return to_input(&Frame {
            format: PixelFormat::Rgb,
            width,
            height,
            stride: width * 3,
            data: image.as_raw(),
        });
    }
    let sampler = Sampler::new(frame)?;
    let plane = INPUT_WIDTH * INPUT_HEIGHT;
    let mut out = vec![0.0; plane * 3];
    for y in 0..INPUT_HEIGHT {
        let sy = y * frame.height / INPUT_HEIGHT;
        for x in 0..INPUT_WIDTH {
            let sx = x * frame.width / INPUT_WIDTH;
            let rgb = sampler.rgb(sx, sy);
            let i = y * INPUT_WIDTH + x;
            for (c, v) in rgb.into_iter().enumerate() {
                out[c * plane + i] = (f32::from(v) - 127.0) / 128.0;
            }
        }
    }
    Some(out)
}

struct Sampler<'a> {
    frame: &'a Frame<'a>,
    stride: usize,
    /// Düzlemli biçimlerde U (ya da UV) ve V düzlemlerinin başlangıcı.
    u: usize,
    v: usize,
}

impl<'a> Sampler<'a> {
    /// Biçime göre gereken veri boyunu denetler.
    fn new(frame: &'a Frame<'a>) -> Option<Self> {
        let (w, h) = (frame.width, frame.height);
        let packed = |bpp: usize| if frame.stride == 0 { w * bpp } else { frame.stride };
        let (stride, u, v, needed) = match frame.format {
            PixelFormat::Yuyv => {
                let s = packed(2);
                (s, 0, 0, s * h)
            }
            PixelFormat::Gray => {
                let s = packed(1);
                (s, 0, 0, s * h)
            }
            PixelFormat::Rgb | PixelFormat::Bgr => {
                let s = packed(3);
                (s, 0, 0, s * h)
            }
            PixelFormat::Nv12 => {
                let s = packed(1);
                (s, s * h, 0, s * h + s * h.div_ceil(2))
            }
            PixelFormat::I420 => {
                let s = packed(1);
                let chroma = s.div_ceil(2) * h.div_ceil(2);
                (s, s * h, s * h + chroma, s * h + 2 * chroma)
            }
            PixelFormat::Mjpeg => return None,
        };
        (frame.data.len() >= needed).then_some(Self { frame, stride, u, v })
    }

    fn rgb(&self, x: usize, y: usize) -> [u8; 3] {
        let d = self.frame.data;
        let s = self.stride;
        match self.frame.format {
            PixelFormat::Yuyv => {
                let pair = y * s + (x & !1) * 2;
                let luma = d[pair + (x & 1) * 2];
                yuv(luma, d[pair + 1], d[pair + 3])
            }
            PixelFormat::Nv12 => {
                let uv = self.u + (y / 2) * s + (x & !1);
                yuv(d[y * s + x], d[uv], d[uv + 1])
            }
            PixelFormat::I420 => {
                let cs = s.div_ceil(2);
                let c = (y / 2) * cs + x / 2;
                yuv(d[y * s + x], d[self.u + c], d[self.v + c])
            }
            PixelFormat::Gray => {
                let g = d[y * s + x];
                [g, g, g]
            }
            PixelFormat::Rgb => {
                let p = y * s + x * 3;
                [d[p], d[p + 1], d[p + 2]]
            }
            PixelFormat::Bgr => {
                let p = y * s + x * 3;
                [d[p + 2], d[p + 1], d[p]]
            }
            PixelFormat::Mjpeg => [0; 3],
        }
    }
}

/// BT.601, sınırlı aralık (kameraların çoğu).
fn yuv(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = 1.164 * (f32::from(y) - 16.0);
    let (d, e) = (f32::from(u) - 128.0, f32::from(v) - 128.0);
    let clamp = |x: f32| x.round().clamp(0.0, 255.0) as u8;
    [clamp(c + 1.596 * e), clamp(c - 0.392 * d - 0.813 * e), clamp(c + 2.017 * d)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Girdinin (x, y) pikselinin RGB değeri.
    fn pixel(input: &[f32], x: usize, y: usize) -> [u8; 3] {
        let plane = INPUT_WIDTH * INPUT_HEIGHT;
        let i = y * INPUT_WIDTH + x;
        [0, 1, 2].map(|c| (input[c * plane + i] * 128.0 + 127.0).round() as u8)
    }

    fn close(a: [u8; 3], b: [u8; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 3)
    }

    const WHITE_YUV: [u8; 3] = [235, 128, 128];
    // Saf kırmızı, BT.601 sınırlı aralık.
    const RED_YUV: [u8; 3] = [81, 90, 240];

    /// Sol yarısı kırmızı, sağ yarısı beyaz 640×480 kareler.
    fn halves(format: PixelFormat) -> Vec<u8> {
        let (w, h) = (640, 480);
        let color = |x: usize| if x < w / 2 { RED_YUV } else { WHITE_YUV };
        match format {
            PixelFormat::Yuyv => (0..h)
                .flat_map(|_| {
                    (0..w).step_by(2).flat_map(move |x| {
                        let [y, u, v] = color(x);
                        [y, u, y, v]
                    })
                })
                .collect(),
            PixelFormat::Nv12 | PixelFormat::I420 => {
                let mut data: Vec<u8> = (0..h).flat_map(|_| (0..w).map(|x| color(x)[0])).collect();
                let row = |c: usize| (0..w).step_by(2).map(move |x| color(x)[c]);
                if format == PixelFormat::Nv12 {
                    for _ in 0..h / 2 {
                        data.extend(row(1).zip(row(2)).flat_map(|(u, v)| [u, v]));
                    }
                } else {
                    for c in [1, 2] {
                        for _ in 0..h / 2 {
                            data.extend(row(c));
                        }
                    }
                }
                data
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn converts_yuv_formats() {
        for format in [PixelFormat::Yuyv, PixelFormat::Nv12, PixelFormat::I420] {
            let data = halves(format);
            let frame = Frame { format, width: 640, height: 480, stride: 0, data: &data };
            let input = to_input(&frame).expect("çevrilmeli");
            assert_eq!(input.len(), 3 * INPUT_WIDTH * INPUT_HEIGHT);
            assert!(close(pixel(&input, 10, 100), [255, 0, 0]), "{format:?} sol kırmızı olmalı");
            assert!(close(pixel(&input, 300, 100), [255, 255, 255]), "{format:?} sağ beyaz olmalı");
        }
    }

    #[test]
    fn converts_rgb_bgr_gray_and_jpeg() {
        let rgb = [10u8, 20, 30].repeat(4 * 2);
        let frame = Frame { format: PixelFormat::Rgb, width: 4, height: 2, stride: 0, data: &rgb };
        assert_eq!(pixel(&to_input(&frame).unwrap(), 0, 0), [10, 20, 30]);
        let frame = Frame { format: PixelFormat::Bgr, ..frame };
        assert_eq!(pixel(&to_input(&frame).unwrap(), 0, 0), [30, 20, 10]);
        let gray = [77u8; 8];
        let frame =
            Frame { format: PixelFormat::Gray, width: 4, height: 2, stride: 0, data: &gray };
        assert_eq!(pixel(&to_input(&frame).unwrap(), 5, 5), [77, 77, 77]);

        let mut jpeg = Vec::new();
        image::RgbImage::from_pixel(64, 48, image::Rgb([200, 30, 30]))
            .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        let frame =
            Frame { format: PixelFormat::Mjpeg, width: 64, height: 48, stride: 0, data: &jpeg };
        assert!(close(pixel(&to_input(&frame).unwrap(), 160, 120), [200, 30, 30]));
    }

    #[test]
    fn honours_stride_and_rejects_short_frames() {
        // Satır sonunda 4 bayt dolgu olan 2×2 RGB kare.
        let data = [1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 7, 8, 9, 10, 11, 12, 0, 0, 0, 0];
        let frame =
            Frame { format: PixelFormat::Rgb, width: 2, height: 2, stride: 10, data: &data };
        let input = to_input(&frame).unwrap();
        assert_eq!(pixel(&input, 0, INPUT_HEIGHT - 1), [7, 8, 9]);
        let frame = Frame { data: &data[..12], ..frame };
        assert!(to_input(&frame).is_none());
        let frame = Frame { format: PixelFormat::Mjpeg, data: b"not a jpeg", ..frame };
        assert!(to_input(&frame).is_none());
    }
}
