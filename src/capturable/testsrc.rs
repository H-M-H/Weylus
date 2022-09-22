use crate::capturable::{Capturable, Geometry, Recorder};
use crate::video::PixelProvider;
use std::error::Error;

#[derive(Debug, Clone, Copy)]
pub struct TestCapturable {
    pub width: usize,
    pub height: usize,
}

pub struct TestRecorder {
    capturable: TestCapturable,
    buf: Vec<u8>,
    i: usize,
}

fn set_default_pixel(buf: &mut [u8], w: usize, x: usize, y: usize) {
    let pos = (x + y * w) * 4;
    let i = x * 8 / w;
    buf[pos] = if i & 1 != 0 { 255 } else { 0 };
    buf[pos + 1] = if i & 2 != 0 { 255 } else { 0 };
    buf[pos + 2] = if i & 4 != 0 { 255 } else { 0 };
}

impl TestRecorder {
    fn new(capturable: TestCapturable) -> Self {
        let mut buf = vec![0; capturable.width * capturable.height * 4];
        let buf_ref = buf.as_mut();
        for y in 0..capturable.height {
            for x in 0..capturable.width {
                set_default_pixel(buf_ref, capturable.width, x, y);
            }
        }
        Self {
            capturable,
            buf,
            i: 0,
        }
    }
}

impl Capturable for TestCapturable {
    fn name(&self) -> String {
        format!("Test Source {}x{}", self.width, self.height)
    }
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        Ok(Geometry::Relative(0.0, 0.0, 1.0, 1.0))
    }
    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn recorder(&self, _: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(TestRecorder::new(*self)))
    }
}

impl Recorder for TestRecorder {
    fn capture(&mut self) -> Result<PixelProvider, Box<dyn Error>> {
        const N: usize = 120;
        let dh = self.capturable.height / N;
        let buf_ref = self.buf.as_mut();
        let w = self.capturable.width;
        for y in self.i * dh..(self.i + 1) * dh {
            for x in 0..w {
                set_default_pixel(buf_ref, w, x, y);
            }
        }
        self.i = (self.i + 1) % N;
        for y in self.i * dh..(self.i + 1) * dh {
            for x in 0..w {
                let pos = (x + y * w) * 4;
                buf_ref[pos] = ((self.i + N * x / w) % N * 256 / N) as u8;
                buf_ref[pos + 1] = ((self.i + N * x / w + N / 3) % N * 256 / N) as u8;
                buf_ref[pos + 2] = ((self.i + N * x / w + 2 * N / 3) % N * 256 / N) as u8;
            }
        }
        Ok(PixelProvider::BGR0(
            self.capturable.width,
            self.capturable.height,
            self.buf.as_slice(),
        ))
    }
}
