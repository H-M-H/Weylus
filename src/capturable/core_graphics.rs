use std::boxed::Box;
use std::error::Error;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use core_foundation::{
    array::CFArray,
    base::{TCFType, ToVoid},
    data::CFData,
    dictionary::{CFDictionary, CFDictionaryRef},
    number::{CFNumber, CFNumberRef},
    string::{CFString, CFStringRef},
};
use core_graphics::{
    display,
    display::{CGDisplay, CGRect},
    image::CGImage,
    window,
    window::CGWindowID,
};

use crate::capturable::{Capturable, Geometry, Recorder};

#[derive(Debug)]
pub struct CGError(String);

impl std::fmt::Display for CGError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for CGError {}

#[derive(Clone)]
pub struct CGDisplayCapturable {
    display: CGDisplay,
}

impl CGDisplayCapturable {
    pub fn new(display: CGDisplay) -> Self {
        Self { display }
    }
}

impl Capturable for CGDisplayCapturable {
    fn name(&self) -> String {
        format!(
            "Monitor (CG, {}x{})",
            self.display.pixels_wide(),
            self.display.pixels_high()
        )
    }
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        let bounds = self.display.bounds();
        let (x0, y0, w, h) = screen_coordsys()?;
        Ok(Geometry::Relative(
            (bounds.origin.x - x0) / w,
            (bounds.origin.y - y0) / h,
            bounds.size.width / w,
            bounds.size.height / h,
        ))
    }
    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn recorder(&self, capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(RecorderCGDisplay::new(
            self.display,
            capture_cursor,
        )))
    }
}

pub struct RecorderCGDisplay {
    img_data: Option<CFData>,
    display: CGDisplay,
    capture_cursor: bool,
}

impl RecorderCGDisplay {
    pub fn new(display: CGDisplay, capture_cursor: bool) -> Self {
        Self {
            img_data: None,
            display,
            capture_cursor,
        }
    }
}

fn check_pixelformat(img: &CGImage) -> Result<(), Box<dyn Error>> {
    // for now assume that the pixels are always in BGR0 format
    // do some basic checks to verify this
    if img.bits_per_pixel() != 32 {
        Err(CGError(format!(
            "Only BGR0 with 32 bits per pixel is supported, not {} bits!",
            img.bits_per_pixel()
        )))?
    }
    if img.bits_per_component() != 8 {
        Err(CGError(format!(
            "Only BGR0 with 8 bits per component is supported, not {} bits!",
            img.bits_per_component()
        )))?
    }
    Ok(())
}

impl Recorder for RecorderCGDisplay {
    fn capture(&mut self) -> Result<crate::video::PixelProvider, Box<dyn Error>> {
        let img = if self.capture_cursor {
            CGDisplay::screenshot(self.display.bounds(), 0, 0, 0)
        } else {
            self.display.image()
        };
        if let Some(img) = img {
            check_pixelformat(&img)?;
            let w = img.width() as usize;
            let h = img.height() as usize;

            // extract raw image data
            self.img_data = Some(img.data());
            Ok(crate::video::PixelProvider::BGR0S(
                w,
                h,
                img.bytes_per_row(),
                self.img_data.as_ref().unwrap().bytes(),
            ))
        } else {
            Err(Box::new(CGError(
                "Failed to capture screen using CoreGraphics.".into(),
            )))
        }
    }
}

#[derive(Clone)]
pub struct CGWindowCapturable {
    id: CGWindowID,
    name: String,
    cursor_id: CGWindowID,
    bounds: CGRect,
    geometry_relative: (f64, f64, f64, f64),
    last_geometry_update: Instant,
}

impl CGWindowCapturable {
    fn update_geometry(&mut self) -> Result<(), Box<dyn Error>> {
        if Instant::now() - self.last_geometry_update > Duration::from_secs(1) {
            self.bounds = get_window_infos()
                .iter()
                .find(|w| w.id == self.id)
                .ok_or_else(|| {
                    CGError(format!(
                        "Could not find information for current window {}.",
                        self.id
                    ))
                })?
                .bounds;
            let (x0, y0, w, h) = screen_coordsys()?;
            self.geometry_relative = (
                (self.bounds.origin.x - x0) / w,
                (self.bounds.origin.y - y0) / h,
                self.bounds.size.width / w,
                self.bounds.size.height / h,
            );
            self.last_geometry_update = Instant::now();
        }
        Ok(())
    }
}

impl Capturable for CGWindowCapturable {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        let (x, y, w, h) = self.geometry_relative;
        Ok(Geometry::Relative(x, y, w, h))
    }
    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        self.update_geometry()
    }
    fn recorder(&self, capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(RecorderCGWindow {
            img_data: None,
            capture_cursor,
            win: self.clone(),
        }))
    }
}
pub struct RecorderCGWindow {
    img_data: Option<CFData>,
    capture_cursor: bool,
    win: CGWindowCapturable,
}

impl Recorder for RecorderCGWindow {
    fn capture(&mut self) -> Result<crate::video::PixelProvider, Box<dyn Error>> {
        self.win.update_geometry()?;
        let img = CGDisplay::screenshot_from_windows(
            self.win.bounds,
            if self.capture_cursor {
                CFArray::from_copyable(&[
                    self.win.cursor_id as *const c_void,
                    self.win.id as *const c_void,
                ])
            } else {
                CFArray::from_copyable(&[self.win.id as *const c_void])
            },
            0,
        );
        if let Some(img) = img {
            check_pixelformat(&img)?;
            let w = img.width() as usize;
            let h = img.height() as usize;

            // extract raw image data
            self.img_data = Some(img.data());
            Ok(crate::video::PixelProvider::BGR0S(
                w,
                h,
                img.bytes_per_row(),
                self.img_data.as_ref().unwrap().bytes(),
            ))
        } else {
            Err(Box::new(CGError(
                "Failed to capture window using CoreGraphics.".into(),
            )))
        }
    }
}

#[derive(Debug)]
struct WindowInfo {
    pub id: CGWindowID,
    pub name: String,
    pub bounds: CGRect,
}

fn get_window_infos() -> Vec<WindowInfo> {
    let mut win_infos = vec![];
    let wins = CGDisplay::window_list_info(
        display::kCGWindowListExcludeDesktopElements | display::kCGWindowListOptionOnScreenOnly,
        None,
    );
    if let Some(wins) = wins {
        for w in wins.iter() {
            let w: CFDictionary<*const c_void, *const c_void> =
                unsafe { CFDictionary::wrap_under_get_rule(*w as CFDictionaryRef) };
            let id = w.get(unsafe { window::kCGWindowNumber }.to_void());
            let id = unsafe { CFNumber::wrap_under_get_rule(*id as CFNumberRef) }
                .to_i64()
                .unwrap() as CGWindowID;

            let bounds = w.get(unsafe { window::kCGWindowBounds }.to_void());
            let bounds = unsafe { CFDictionary::wrap_under_get_rule(*bounds as CFDictionaryRef) };
            let bounds = CGRect::from_dict_representation(&bounds).unwrap();

            let name = w.find(unsafe { window::kCGWindowName }.to_void());
            if let None = name {
                continue;
            }
            let name = name.unwrap();
            let name = unsafe { CFString::wrap_under_get_rule(*name as CFStringRef) };
            win_infos.push(WindowInfo {
                id,
                name: name.to_string(),
                bounds,
            });
        }
    }
    win_infos
}

pub fn screen_coordsys() -> Result<(f64, f64, f64, f64), Box<dyn Error>> {
    let display_ids = CGDisplay::active_displays()
        .map_err(|err| CGError(format!("Failed to obtain displays, CGError code: {}", err)))?;
    let rects: Vec<CGRect> = display_ids
        .iter()
        .map(|id| CGDisplay::new(*id).bounds())
        .collect();
    let mut x0 = 0.0;
    let mut x1 = 0.0;
    let mut y0 = 0.0;
    let mut y1 = 0.0;
    for r in rects.iter() {
        let r_x0 = r.origin.x;
        let r_x1 = r_x0 + r.size.width;
        let r_y0 = r.origin.y;
        let r_y1 = r_y0 + r.size.height;
        x0 = f64::min(x0, r_x0);
        x1 = f64::max(x1, r_x1);
        y0 = f64::min(y0, r_y0);
        y1 = f64::max(y1, r_y1);
    }
    Ok((x0, y0, x1 - x0, y1 - y0))
}

pub fn get_displays() -> Result<Vec<CGDisplayCapturable>, Box<dyn Error>> {
    let display_ids = CGDisplay::active_displays()
        .map_err(|err| CGError(format!("Failed to obtain displays, CGError code: {}", err)))?;
    Ok(display_ids
        .iter()
        .map(|id| CGDisplayCapturable::new(CGDisplay::new(*id)))
        .collect())
}

pub fn get_windows() -> Result<Vec<CGWindowCapturable>, Box<dyn Error>> {
    let window_infos = get_window_infos();
    let cursor_id = window_infos
        .iter()
        .find(|w| w.name == "Cursor")
        .ok_or_else(|| CGError("No Cursor found!".into()))?
        .id;
    Ok(window_infos
        .iter()
        .filter(|w| w.id != cursor_id)
        .map(|w| CGWindowCapturable {
            id: w.id,
            name: w.name.clone(),
            cursor_id,
            bounds: w.bounds,
            geometry_relative: (0.0, 0.0, 1.0, 1.0),
            last_geometry_update: Instant::now() - Duration::from_secs(2),
        })
        .collect())
}
