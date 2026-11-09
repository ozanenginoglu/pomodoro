//! Windows: Media Foundation, nokhwa üzerinden. Kareler ham alınır ve
//! `frame` modülünde çevrilir (nokhwa'nın çözücüleri kapalı).

use anyhow::Context;
use nokhwa::{
    Buffer, Camera,
    pixel_format::RgbFormat,
    query,
    utils::{ApiBackend, CameraFormat, FrameFormat, RequestedFormat, RequestedFormatType},
};

use super::{
    Device,
    frame::{Frame, PixelFormat},
};

pub fn devices() -> Vec<Device> {
    query(ApiBackend::MediaFoundation)
        .unwrap_or_default()
        .into_iter()
        .map(|info| Device { id: info.misc(), name: info.human_name() })
        .collect()
}

pub struct Capture {
    camera: Camera,
    buffer: Option<Buffer>,
    name: String,
}

impl Capture {
    pub fn open(id: Option<&str>) -> anyhow::Result<Self> {
        let cameras = query(ApiBackend::MediaFoundation).context("kameralar listelenemedi")?;
        let info = match id {
            Some(id) => cameras.iter().find(|c| c.misc() == id),
            None => None,
        }
        .or(cameras.first())
        .context("kamera bulunamadı")?;

        // Önce çevirmesi ucuz biçimlerde 640×480 istenir, olmazsa ne varsa.
        let requests = [FrameFormat::YUYV, FrameFormat::NV12, FrameFormat::MJPEG]
            .map(|f| RequestedFormatType::Closest(CameraFormat::new_from(640, 480, f, 30)))
            .into_iter()
            .chain([RequestedFormatType::AbsoluteHighestFrameRate]);
        let mut last_error = None;
        for request in requests {
            let format = RequestedFormat::new::<RgbFormat>(request);
            match Camera::with_backend(info.index().clone(), format, ApiBackend::MediaFoundation)
                .and_then(|mut camera| camera.open_stream().map(|()| camera))
            {
                Ok(camera) => {
                    let name = format!("{} ({})", info.human_name(), camera.camera_format());
                    return Ok(Self { camera, buffer: None, name });
                }
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error
            .map(anyhow::Error::from)
            .unwrap_or_else(|| anyhow::anyhow!("kamera açılamadı")))
    }

    pub fn describe(&self) -> &str {
        &self.name
    }

    pub fn next_frame(&mut self) -> anyhow::Result<Frame<'_>> {
        let buffer = self.buffer.insert(self.camera.frame().context("kameradan görüntü gelmiyor")?);
        let format = match buffer.source_frame_format() {
            FrameFormat::YUYV => PixelFormat::Yuyv,
            FrameFormat::NV12 => PixelFormat::Nv12,
            FrameFormat::MJPEG => PixelFormat::Mjpeg,
            FrameFormat::GRAY => PixelFormat::Gray,
            FrameFormat::RAWRGB => PixelFormat::Rgb,
            FrameFormat::RAWBGR => PixelFormat::Bgr,
        };
        let resolution = buffer.resolution();
        Ok(Frame {
            format,
            width: resolution.width() as usize,
            height: resolution.height() as usize,
            stride: 0,
            data: buffer.buffer(),
        })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.camera.stop_stream();
    }
}
