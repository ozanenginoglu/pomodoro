//! Yüz algılama: UltraFace (version-RFB-320, ~1.2 MB ONNX), `tract` ile.
//!
//! Kim olduğunu değil, yalnızca bir yüz olup olmadığını ve nerede olduğunu
//! söyler. Prototipte gerçek bir yüzde skor 1.00, boş sahnede en fazla ~0.27
//! çıktı; eşik bu yüzden 0.5. Yandan bakan yüz de bulunuyor.

use std::sync::Arc;

use anyhow::Context;
use tract_onnx::prelude::*;

use super::frame::{INPUT_HEIGHT, INPUT_WIDTH};

static MODEL: &[u8] = include_bytes!("../../assets/models/ultraface-rfb-320.onnx");
const THRESHOLD: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub score: f32,
    /// Yüzün merkezi, karenin genişliğine/yüksekliğine göre 0 ..= 1.
    pub center: [f32; 2],
}

pub struct Detector {
    model: Arc<TypedRunnableModel>,
}

impl Detector {
    pub fn new() -> anyhow::Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL))?
            .with_input_fact(0, f32::fact([1, 3, INPUT_HEIGHT, INPUT_WIDTH]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self { model })
    }

    /// `input`: `frame::to_input` çıktısı. En emin yüzü döndürür.
    pub fn detect(&self, input: Vec<f32>) -> anyhow::Result<Option<Face>> {
        let tensor: Tensor =
            tract_ndarray::Array4::from_shape_vec((1, 3, INPUT_HEIGHT, INPUT_WIDTH), input)
                .context("girdi boyutu")?
                .into();
        let out = self.model.run(tvec!(tensor.into()))?;
        let scores = out[0].to_plain_array_view::<f32>()?;
        let boxes = out[1].to_plain_array_view::<f32>()?;
        let mut best: Option<Face> = None;
        let mut top = 0.0f32;
        for i in 0..scores.shape()[1] {
            let score = scores[[0, i, 1]];
            top = top.max(score);
            if score < THRESHOLD || best.is_some_and(|b| b.score >= score) {
                continue;
            }
            let [x1, y1, x2, y2] = [0, 1, 2, 3].map(|k| boxes[[0, i, k]]);
            best = Some(Face { score, center: [(x1 + x2) / 2.0, (y1 + y2) / 2.0] });
        }
        // Eşiğin altında kalan yüzleri de görmek için (ışık, uzaklık ayarı).
        log::trace!("kamera: en yüksek skor {top:.2}");
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_no_face_in_an_empty_scene() {
        let detector = Detector::new().expect("model yüklenmeli");
        let gray = vec![-0.2; 3 * INPUT_WIDTH * INPUT_HEIGHT];
        assert_eq!(detector.detect(gray).unwrap(), None);
        assert!(detector.detect(vec![0.0; 10]).is_err(), "yanlış boyut reddedilmeli");
    }

    /// NASA'nın kamu malı astronot portresinden (Eileen Collins) bir kesit.
    #[test]
    fn finds_the_face_in_a_portrait() {
        use super::super::frame::{Frame, PixelFormat, to_input};
        let jpeg = include_bytes!("testdata/face.jpg");
        let frame =
            Frame { format: PixelFormat::Mjpeg, width: 160, height: 120, stride: 0, data: jpeg };
        let face = Detector::new().unwrap().detect(to_input(&frame).unwrap()).unwrap();
        let face = face.expect("yüz bulunmalı");
        assert!(face.score > 0.9, "{face:?}");
        // Yüz kesitin üst ortasında.
        assert!((0.3..0.7).contains(&face.center[0]) && face.center[1] < 0.6, "{face:?}");
    }
}
