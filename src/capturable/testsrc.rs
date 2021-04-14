use crate::capturable::{Capturable, Recorder};
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

impl TestRecorder {
    fn new(capturable: TestCapturable) -> Self {
        Self {
            capturable,
            buf: vec![0; capturable.width * capturable.height * 4],
            i: 0,
        }
    }
}

impl Capturable for TestCapturable {
    fn name(&self) -> String {
        format!("Test Source {}x{}", self.width, self.height)
    }
    fn geometry_relative(&self) -> Result<(f64, f64, f64, f64), Box<dyn Error>> {
        Ok((1.0, 1.0, 1.0, 1.0))
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
        let i = self.i;
        let dw = (self.capturable.width / 256 * 2).max(1);
        let dh = (self.capturable.height / 256 * 2).max(1);
        for y in 0..self.capturable.height {
            for x in 0..self.capturable.width {
                let pos = x * y * 4;
                let xr = x / dw;
                let yr = y / dh;
                self.buf[pos] = ((xr + yr + i) % 256) as u8;
                self.buf[pos + 1] = ((xr + yr + i / 2) % 256) as u8;
                self.buf[pos + 2] = ((xr + yr + 2 * i) % 256) as u8;
            }
        }
        self.i += 1;
        Ok(PixelProvider::BGR0(
            self.capturable.width,
            self.capturable.height,
            self.buf.as_slice(),
        ))
    }
}
