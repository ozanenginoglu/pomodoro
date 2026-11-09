//! Linux: V4L2 ile doğrudan görüntü alma.
//!
//! Kameranın desteklediği biçimlerden çevirebildiğimiz ilki seçilir ve
//! 640×480'e en yakın boyut istenir. v4l2loopback gibi biçimi sabit
//! aygıtlarda (ör. DroidCam: 1080p I420) olduğu gibi kullanılır.

use std::time::Duration;

use anyhow::{Context, bail};
use v4l::{
    Device as V4lDevice, FourCC,
    buffer::Type,
    capability::Flags,
    framesize::FrameSizeEnum,
    io::{mmap::Stream, traits::CaptureStream},
    video::Capture as _,
};

use super::{
    Device,
    frame::{Frame, PixelFormat},
};

/// Tercih sırasıyla çevirebildiğimiz biçimler. Sıkıştırılmamışlar önce:
/// çözmeleri MJPEG'den ucuz.
const FORMATS: [(&[u8; 4], PixelFormat); 6] = [
    (b"YUYV", PixelFormat::Yuyv),
    (b"NV12", PixelFormat::Nv12),
    (b"YU12", PixelFormat::I420),
    (b"MJPG", PixelFormat::Mjpeg),
    (b"GREY", PixelFormat::Gray),
    (b"RGB3", PixelFormat::Rgb),
];
const WANTED: (u32, u32) = (640, 480);

fn pixel_format(fourcc: FourCC) -> Option<PixelFormat> {
    FORMATS.iter().find(|(code, _)| FourCC::new(code) == fourcc).map(|&(_, format)| format)
}

pub fn devices() -> Vec<Device> {
    let mut nodes = v4l::context::enum_devices();
    nodes.sort_by_key(|node| node.index());
    nodes
        .into_iter()
        .filter_map(|node| {
            let device = V4lDevice::with_path(node.path()).ok()?;
            let caps = device.query_caps().ok()?;
            // Meta veri düğümleri (ör. /dev/video1) ve çıkış aygıtları elenir.
            caps.capabilities
                .contains(Flags::VIDEO_CAPTURE)
                .then(|| Device { id: node.path().to_string_lossy().into_owned(), name: caps.card })
        })
        .collect()
}

pub struct Capture {
    // `stream` aygıtın dosya tanıtıcısını kendi de tutar; aygıt yine de
    // akış bitene kadar açık kalsın diye burada.
    _device: V4lDevice,
    stream: Stream<'static>,
    format: PixelFormat,
    width: usize,
    height: usize,
    stride: usize,
    name: String,
}

impl Capture {
    pub fn open(id: Option<&str>) -> anyhow::Result<Self> {
        let path = match id {
            Some(id) => id.to_string(),
            None => devices().into_iter().next().context("kamera bulunamadı")?.id,
        };
        let device = V4lDevice::with_path(&path).with_context(|| format!("{path} açılamadı"))?;
        let name = device.query_caps().map(|c| c.card).unwrap_or_else(|_| path.clone());

        let mut format = device.format().context("biçim okunamadı")?;
        if let Some((fourcc, (width, height))) = choose_format(&device) {
            format.fourcc = fourcc;
            format.width = width;
            format.height = height;
            // Sabit biçimli aygıtlar isteği yok sayıp kendi biçimini döndürür.
            format = device.set_format(&format).unwrap_or(format);
        }
        let Some(pixel) = pixel_format(format.fourcc) else {
            bail!("{name}: desteklenmeyen piksel biçimi {}", format.fourcc);
        };

        let mut stream = Stream::with_buffers(&device, Type::VideoCapture, 4)
            .with_context(|| format!("{name}: görüntü akışı başlatılamadı"))?;
        stream.set_timeout(Duration::from_secs(3));
        Ok(Self {
            _device: device,
            stream,
            format: pixel,
            width: format.width as usize,
            height: format.height as usize,
            stride: format.stride as usize,
            name: format!("{name} ({path}, {}x{} {})", format.width, format.height, format.fourcc),
        })
    }

    pub fn describe(&self) -> &str {
        &self.name
    }

    /// Sıradaki kare. Kare gelmezse (aygıt çıkarıldı, kaynak durdu) hata.
    pub fn next_frame(&mut self) -> anyhow::Result<Frame<'_>> {
        let (data, meta) = self.stream.next().context("kameradan görüntü gelmiyor")?;
        let used = (meta.bytesused as usize).min(data.len());
        let data = if used > 0 { &data[..used] } else { data };
        Ok(Frame {
            format: self.format,
            width: self.width,
            height: self.height,
            stride: if self.format == PixelFormat::Mjpeg { 0 } else { self.stride },
            data,
        })
    }
}

/// Desteklenen ilk biçim ve onun 640×480'e en yakın boyutu.
fn choose_format(device: &V4lDevice) -> Option<(FourCC, (u32, u32))> {
    let available: Vec<FourCC> =
        device.enum_formats().ok()?.into_iter().map(|d| d.fourcc).collect();
    let fourcc = FORMATS
        .iter()
        .map(|(code, _)| FourCC::new(code))
        .find(|fourcc| available.contains(fourcc))?;
    let distance = |(w, h): (u32, u32)| w.abs_diff(WANTED.0) + h.abs_diff(WANTED.1);
    let size = device
        .enum_framesizes(fourcc)
        .ok()?
        .into_iter()
        .flat_map(|size| match size.size {
            FrameSizeEnum::Discrete(d) => vec![(d.width, d.height)],
            FrameSizeEnum::Stepwise(s) => vec![(
                WANTED.0.clamp(s.min_width, s.max_width),
                WANTED.1.clamp(s.min_height, s.max_height),
            )],
        })
        .min_by_key(|&size| distance(size))?;
    Some((fourcc, size))
}
