use crate::capturable::{Capturable, Geometry, Recorder};
use crate::video::PixelProvider;
use std::error::Error;

#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    BGR0,
    RGB0,
    RGB,
}

#[derive(Debug, Clone, Copy)]
pub struct TestCapturable {
    pub width: usize,
    pub height: usize,
    pub pixel_format: PixelFormat,
}

impl TestCapturable {
    fn pixel_size(&self) -> usize {
        match self.pixel_format {
            PixelFormat::BGR0 => 4,
            PixelFormat::RGB0 => 4,
            PixelFormat::RGB => 3,
        }
    }
    fn set_default_pixel(&self, buf: &mut [u8], x: usize, y: usize) {
        let w = self.width;
        let i = x * 8 / w;
        let pos = (x + y * w) * self.pixel_size();
        let (pos_r, pos_g, pos_b) = match self.pixel_format {
            PixelFormat::BGR0 => (pos + 2, pos + 1, pos),
            PixelFormat::RGB0 | PixelFormat::RGB => (pos, pos + 1, pos + 2),
        };
        buf[pos_b] = if i & 1 != 0 { 255 } else { 0 };
        buf[pos_g] = if i & 2 != 0 { 255 } else { 0 };
        buf[pos_r] = if i & 4 != 0 { 255 } else { 0 };
    }
}

pub struct TestRecorder {
    capturable: TestCapturable,
    buf: Vec<u8>,
    i: usize,
}

impl TestRecorder {
    fn new(capturable: TestCapturable) -> Self {
        let mut buf = vec![0; capturable.width * capturable.height * capturable.pixel_size()];
        let buf_ref = buf.as_mut();
        for y in 0..capturable.height {
            for x in 0..capturable.width {
                capturable.set_default_pixel(buf_ref, x, y);
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
        format!(
            "Test Source {}x{}@{:?}",
            self.width, self.height, self.pixel_format
        )
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
    fn capture(&mut self) -> Result<PixelProvider<'_>, Box<dyn Error>> {
        const N: usize = 120;
        let dh = self.capturable.height / N;
        let buf_ref = self.buf.as_mut();
        let w = self.capturable.width;
        for y in self.i * dh..(self.i + 1) * dh {
            for x in 0..w {
                self.capturable.set_default_pixel(buf_ref, x, y);
            }
        }
        self.i = (self.i + 1) % N;
        for y in self.i * dh..(self.i + 1) * dh {
            for x in 0..w {
                let pos = (x + y * w) * self.capturable.pixel_size();

                let (pos_r, pos_g, pos_b) = match self.capturable.pixel_format {
                    PixelFormat::BGR0 => (pos + 2, pos + 1, pos),
                    PixelFormat::RGB0 | PixelFormat::RGB => (pos, pos + 1, pos + 2),
                };
                buf_ref[pos_b] = ((self.i + N * x / w) % N * 256 / N) as u8;
                buf_ref[pos_g] = ((self.i + N * x / w + N / 3) % N * 256 / N) as u8;
                buf_ref[pos_r] = ((self.i + N * x / w + 2 * N / 3) % N * 256 / N) as u8;
            }
        }
        Ok(match self.capturable.pixel_format {
            PixelFormat::BGR0 => PixelProvider::BGR0(
                self.capturable.width,
                self.capturable.height,
                self.buf.as_slice(),
            ),
            PixelFormat::RGB0 => PixelProvider::RGB0(
                self.capturable.width,
                self.capturable.height,
                self.buf.as_slice(),
            ),
            PixelFormat::RGB => PixelProvider::RGB(
                self.capturable.width,
                self.capturable.height,
                self.buf.as_slice(),
            ),
        })
    }
}
