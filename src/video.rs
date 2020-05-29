use std::os::raw::{c_int, c_uchar, c_void};
use std::time::Instant;

use crate::cerror::CError;

extern "C" {
    fn init_video_encoder(rust_ctx: *mut c_void, width: c_int, height: c_int) -> *mut c_void;
    fn open_video(handle: *mut c_void, err: *mut CError);
    fn destroy_video_encoder(handle: *mut c_void);
    fn get_video_frame_data(
        handle: *const c_void,
        y_linesize: *mut c_int,
        u_linesize: *mut c_int,
        v_linesize: *mut c_int,
    ) -> *const *mut u8;
    fn encode_video_frame(handle: *mut c_void, millis: c_int, err: *mut CError);
}

#[no_mangle]
fn write_video_packet(video_encoder: *mut c_void, buf: *const c_uchar, buf_size: c_int) -> c_int {
    let video_encoder = unsafe { (video_encoder as *mut VideoEncoder).as_mut().unwrap() };
    (video_encoder.write_data)(unsafe {
        std::slice::from_raw_parts(buf as *const u8, buf_size as usize)
    });
    return 0;
}

pub struct VideoEncoder {
    handle: *mut c_void,
    width: usize,
    height: usize,
    write_data: Box<dyn Fn(&[u8])>,
    start_time: Instant,
}

impl VideoEncoder {
    pub fn new(
        width: usize,
        height: usize,
        write_data: impl Fn(&[u8]) + 'static,
    ) -> Result<Box<Self>, CError> {
        // yuv420p only supports even width and height
        let width = width - width % 2;
        let height = height - height % 2;
        let mut video_encoder = Box::new(Self {
            handle: std::ptr::null_mut(),
            width,
            height,
            write_data: Box::new(move |data| write_data(data)),
            start_time: Instant::now(),
        });
        let handle = unsafe {
            init_video_encoder(
                video_encoder.as_mut() as *mut _ as *mut c_void,
                width as c_int,
                height as c_int,
            )
        };
        video_encoder.handle = handle;

        let mut err = CError::new();
        unsafe { open_video(video_encoder.handle, &mut err) };
        if err.is_err() {
            return Err(err);
        }
        Ok(video_encoder)
    }

    pub fn encode(
        &mut self,
        fill_yuv: impl Fn(&mut [u8], &mut [u8], &mut [u8], usize, usize, usize),
    ) {
        let mut y_linesize: c_int = 0;
        let mut u_linesize: c_int = 0;
        let mut v_linesize: c_int = 0;
        let data = unsafe {
            get_video_frame_data(
                self.handle,
                &mut y_linesize,
                &mut u_linesize,
                &mut v_linesize,
            )
        };
        let y_linesize = y_linesize as usize;
        let u_linesize = u_linesize as usize;
        let v_linesize = v_linesize as usize;
        let data = unsafe { std::slice::from_raw_parts(data, 3) };
        let y = unsafe { std::slice::from_raw_parts_mut(data[0], y_linesize * self.height) };
        let u = unsafe { std::slice::from_raw_parts_mut(data[1], u_linesize * self.height) };
        let v = unsafe { std::slice::from_raw_parts_mut(data[2], v_linesize * self.height) };
        fill_yuv(y, u, v, y_linesize, u_linesize, v_linesize);
        let mut err = CError::new();
        unsafe { encode_video_frame(self.handle, (Instant::now() - self.start_time).as_millis() as c_int, &mut err) };
    }

    pub fn check_size(&self, width: usize, height: usize) -> bool {
        (self.width == width - width % 2) && (self.height == height - height % 2)
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { destroy_video_encoder(self.handle) }
        }
    }
}
