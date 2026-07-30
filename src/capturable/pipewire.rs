use std::collections::HashMap;
use std::error::Error;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, trace, warn};

use dbus::{
    arg::{OwnedFd, PropMap, RefArg, Variant},
    blocking::{Proxy, SyncConnection},
    message::{MatchRule, MessageType},
    Message,
};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;

use crate::capturable::{Capturable, Geometry, Recorder};
use crate::config::PipewirePipeline;
use crate::video::PixelProvider;

use crate::capturable::remote_desktop_dbus::{
    OrgFreedesktopPortalRemoteDesktop, OrgFreedesktopPortalRequestResponse,
    OrgFreedesktopPortalScreenCast,
};

#[derive(Debug, Clone, Copy)]
struct PwStreamInfo {
    path: u64,
    source_type: u64,
    /// Portal `position` (ii): the stream's (x, y) in the compositor's logical
    /// coordinate space. Monitor streams only; `None` for windows / when absent.
    position: Option<(i32, i32)>,
    /// Portal `size` (ii): the stream's (width, height) in logical space.
    size: Option<(i32, i32)>,
}

use crate::capturable::wayland_outputs::GlobalBox;

/// Extract a D-Bus `(ii)` value (as the portal reports `position`/`size`) into `(i32, i32)`.
///
/// The value arrives wrapped in a Variant around a two-element struct, so we descend
/// through any container layers and collect the integer leaves; a genuine `(ii)` yields
/// exactly two.
fn extract_ii(arg: &dyn RefArg) -> Option<(i32, i32)> {
    fn collect_ints(arg: &dyn RefArg, out: &mut Vec<i64>) {
        if let Some(i) = arg.as_i64() {
            out.push(i);
            return;
        }
        if let Some(it) = arg.as_iter() {
            for x in it {
                collect_ints(x, out);
            }
        }
    }
    let mut ints = Vec::new();
    collect_ints(arg, &mut ints);
    match ints.as_slice() {
        [a, b] => Some((*a as i32, *b as i32)),
        _ => None,
    }
}

/// Compute the `Geometry::Relative` for a monitor stream whose logical rect is
/// `(px, py, sw, sh)`, normalised against the global bounding box `gbox` (the space the
/// compositor decodes tablet ABS axes against). Returns whole-screen `(0,0,1,1)` when the
/// stream has no usable geometry (e.g. a window, which the portal reports as a dummy
/// `1x1`) or when the global box is unknown/degenerate.
fn relative_geometry(
    source_type: u64,
    position: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
    gbox: Option<GlobalBox>,
) -> Geometry {
    // source_type 1 == MONITOR; windows (2) report bogus position/size.
    if source_type != 1 {
        return Geometry::Relative(0.0, 0.0, 1.0, 1.0);
    }
    match (position, size, gbox) {
        (Some((px, py)), Some((sw, sh)), Some(g))
            if g.width > 0 && g.height > 0 && sw > 1 && sh > 1 =>
        {
            let w = g.width as f64;
            let h = g.height as f64;
            Geometry::Relative(
                (px - g.x) as f64 / w,
                (py - g.y) as f64 / h,
                sw as f64 / w,
                sh as f64 / h,
            )
        }
        _ => Geometry::Relative(0.0, 0.0, 1.0, 1.0),
    }
}

#[derive(Debug)]
pub struct DBusError(String);

impl std::fmt::Display for DBusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for DBusError {}

#[derive(Debug)]
pub struct GStreamerError(String);

impl std::fmt::Display for GStreamerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for GStreamerError {}

#[derive(Clone)]
pub struct PipeWireCapturable {
    // connection needs to be kept alive for recording
    dbus_conn: Arc<SyncConnection>,
    fd: OwnedFd,
    path: u64,
    source_type: u64,
    /// Stream's logical (x, y) from the portal; used to map stylus input.
    position: Option<(i32, i32)>,
    /// Stream's logical (width, height) from the portal.
    size: Option<(i32, i32)>,
    /// Global bounding box of all outputs (logical space), queried once at
    /// enumeration time. `None` when Wayland output geometry is unavailable, in
    /// which case `geometry()` falls back to the whole screen.
    global_box: Option<GlobalBox>,
    pipeline: PipewirePipeline,
}

impl PipeWireCapturable {
    fn new(
        conn: Arc<SyncConnection>,
        fd: OwnedFd,
        stream: PwStreamInfo,
        global_box: Option<GlobalBox>,
        pipeline: PipewirePipeline,
    ) -> Self {
        Self {
            dbus_conn: conn,
            fd,
            path: stream.path,
            source_type: stream.source_type,
            position: stream.position,
            size: stream.size,
            global_box,
            pipeline,
        }
    }
}

impl std::fmt::Debug for PipeWireCapturable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PipeWireCapturable {{dbus: {}, fd: {}, path: {}, source_type: {}, \
             position: {:?}, size: {:?}, global_box: {:?}, pipeline: {:?}}}",
            self.dbus_conn.unique_name(),
            self.fd.as_raw_fd(),
            self.path,
            self.source_type,
            self.position,
            self.size,
            self.global_box,
            self.pipeline,
        )
    }
}

impl Capturable for PipeWireCapturable {
    fn name(&self) -> String {
        let type_str = match self.source_type {
            1 => "Desktop",
            2 => "Window",
            _ => "Unknown",
        };
        format!("Pipewire {}, path: {}", type_str, self.path)
    }

    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        Ok(relative_geometry(
            self.source_type,
            self.position,
            self.size,
            self.global_box,
        ))
    }

    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn recorder(
        &self,
        _capture_cursor: bool,
        prefer_dmabuf: bool,
    ) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(PipeWireRecorder::new(self.clone(), prefer_dmabuf)?))
    }
}

// Non-CCS XR24 modifiers (LINEAR + X_TILED + Y_TILED), safe for both VA import
// (zero-copy dmabuf path) and GL de-tile. Excludes niri's default CCS modifier
// which is multi-plane and rejected by single-plane consumers.
const XR24_NONCCS: &str = "video/x-raw(memory:DMABuf), format=(string)DMA_DRM, \
     width=(int)[1,32767], height=(int)[1,32767], \
     framerate=(fraction)[0/1,2147483647/1], drm-format=(list){ \
     (string)XR24, (string)XR24:0x0100000000000001, (string)XR24:0x0100000000000002 }";
// LINEAR only (bare fourcc = DRM_FORMAT_MOD_LINEAR), for the cheap-readback CPU
// path: no de-tile needed on download.
const XR24_LINEAR: &str = "video/x-raw(memory:DMABuf), format=(string)DMA_DRM, \
     width=(int)[1,32767], height=(int)[1,32767], \
     framerate=(fraction)[0/1,2147483647/1], drm-format=(string)XR24";

/// Parse the DRM modifier out of a negotiated `drm-format` caps value, e.g.
/// `"XR24:0x0100000000000002"` -> `0x0100000000000002` (Y_TILED). A bare fourcc
/// (`"XR24"`) or an unparseable suffix means LINEAR (`0`). This MUST be threaded
/// into the DRM-PRIME descriptor: if the buffer is tiled but we tell VAAPI it is
/// linear, the import reads tiled memory as linear and the frame comes out
/// spatially scrambled.
fn parse_drm_modifier(drm_format: &str) -> u64 {
    match drm_format.split_once(':') {
        Some((_, m)) => {
            let m = m.trim();
            let hex = m.strip_prefix("0x").or_else(|| m.strip_prefix("0X")).unwrap_or(m);
            u64::from_str_radix(hex, 16).unwrap_or(0)
        }
        None => 0,
    }
}

pub struct PipeWireRecorder {
    buffer: Option<gst::MappedBuffer<gst::buffer::Readable>>,
    buffer_cropped: Vec<u8>,
    pix_fmt: String,
    is_cropped: bool,
    pipeline: gst::Pipeline,
    appsink: AppSink,
    width: usize,
    height: usize,
    // Zero-copy dmabuf path: when set, `capture()` returns `PixelProvider::DmaBuf`
    // instead of a mapped CPU buffer.
    is_dmabuf: bool,
    // Retained backing buffer (holds the fd alive) + extracted DRM layout:
    // (buffer, width, height, stride, offset, fd, modifier).
    dmabuf: Option<(gst::Buffer, usize, usize, u32, u32, i32, u64)>,
}

impl PipeWireRecorder {
    pub fn new(
        capturable: PipeWireCapturable,
        prefer_dmabuf: bool,
    ) -> Result<Self, Box<dyn Error>> {
        // Two pipelines can turn a PipeWire screen-cast stream into the system-memory
        // `BGRx`/`RGBx` the appsink (and x264 encoder) need:
        //
        //  * DIRECT  `pipewiresrc -> appsink` -- the historic path. Works only where the
        //    compositor/backend hands back CPU-mappable buffers (system memory, or a
        //    linear dmabuf pipewiresrc can map). Cheapest: no GPU, no colour convert.
        //  * GL      `pipewiresrc -> capsfilter -> glupload -> glcolorconvert ->
        //    gldownload -> videoconvert -> appsink` -- required for compositors that only
        //    offer tiled/modified DMA-BUF (niri, modern sway/KDE/GNOME). Imports the
        //    dmabuf on the GPU and downloads it. Costs an EGL import + GPU convert + a
        //    read-back per frame, and needs a working GL/EGL context.
        //
        // The strategy comes from the GUI / config (`--pipewire-pipeline`), stored on the
        // capturable. The `WEYLUS_PIPEWIRE_PIPELINE` env var still overrides it for quick
        // debugging without touching config.
        //    Auto   -- try DIRECT, fall back to GL if it fails to negotiate.
        //    Direct -- force DIRECT.
        //    Gl     -- force GL.
        // `Auto` keeps the direct path (and its zero overhead) on setups where it always
        // worked, and only pays the GL cost where the direct path can't negotiate.
        let mode = match std::env::var("WEYLUS_PIPEWIRE_PIPELINE")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "direct" => PipewirePipeline::Direct,
            "gl" => PipewirePipeline::Gl,
            "dmabuf" => PipewirePipeline::Dmabuf,
            "linear-cpu" => PipewirePipeline::LinearCpu,
            "auto" => PipewirePipeline::Auto,
            _ => capturable.pipeline,
        };

        match mode {
            PipewirePipeline::Direct => Self::start(Self::build_direct(&capturable)?),
            PipewirePipeline::Gl => Self::start(Self::build_gl(&capturable)?),
            PipewirePipeline::Dmabuf => {
                let mut r = Self::start(Self::build_dmabuf(&capturable)?)?;
                r.is_dmabuf = true;
                Ok(r)
            }
            PipewirePipeline::LinearCpu => Self::start(Self::build_linear_cpu(&capturable)?),
            PipewirePipeline::Auto => {
                // Probe the cheap direct path first (unchanged).
                let (pipeline, appsink) = Self::build_direct(&capturable)?;
                if Self::try_reach_playing(&pipeline, 3) {
                    return Ok(Self::finish(pipeline, appsink));
                }
                let _ = pipeline.set_state(gst::State::Null);
                debug!(
                    "Direct pipewiresrc path did not negotiate ({}); trying the zero-copy \
                     / GL cascade.",
                    Self::drain_bus_errors(&pipeline)
                );
                // Zero-copy dmabuf -> VAAPI, only when the caller confirmed VAAPI is usable.
                if prefer_dmabuf {
                    if let Ok((p, a)) = Self::build_dmabuf(&capturable) {
                        if Self::try_reach_playing(&p, 3) {
                            let mut r = Self::finish(p, a);
                            r.is_dmabuf = true;
                            return Ok(r);
                        }
                        let _ = p.set_state(gst::State::Null);
                    }
                    debug!("Dmabuf path unavailable; falling back to LinearCpu.");
                }
                if let Ok((p, a)) = Self::build_linear_cpu(&capturable) {
                    if Self::try_reach_playing(&p, 3) {
                        return Ok(Self::finish(p, a));
                    }
                    let _ = p.set_state(gst::State::Null);
                }
                debug!("LinearCpu path unavailable; falling back to GL.");
                Self::start(Self::build_gl(&capturable)?)
            }
        }
    }

    fn make_src(
        capturable: &PipeWireCapturable,
        always_copy: bool,
    ) -> Result<gst::Element, Box<dyn Error>> {
        let src = gst::ElementFactory::make("pipewiresrc").build()?;
        src.set_property("fd", &capturable.fd.as_raw_fd());
        src.set_property("path", &format!("{}", capturable.path));
        src.set_property("always-copy", &always_copy);
        Ok(src)
    }

    fn make_appsink() -> Result<AppSink, Box<dyn Error>> {
        let sink = gst::ElementFactory::make("appsink").build()?;
        sink.set_property("drop", &true);
        sink.set_property("max-buffers", &1u32);
        let appsink = sink
            .dynamic_cast::<AppSink>()
            .map_err(|_| GStreamerError("Sink element is expected to be an appsink!".into()))?;
        let mut caps = gst::Caps::new_empty();
        caps.merge_structure(gst::structure::Structure::from_iter(
            "video/x-raw",
            [("format", "BGRx".into())],
        ));
        caps.merge_structure(gst::structure::Structure::from_iter(
            "video/x-raw",
            [("format", "RGBx".into())],
        ));
        appsink.set_caps(Some(&caps));
        Ok(appsink)
    }

    /// Historic `pipewiresrc -> appsink` path. `always-copy=true` matches the original
    /// behaviour (it also works around a PipeWire teardown hang, pw#982) and is safe here
    /// because this path only negotiates CPU-mappable buffers.
    fn build_direct(
        capturable: &PipeWireCapturable,
    ) -> Result<(gst::Pipeline, AppSink), Box<dyn Error>> {
        let pipeline = gst::Pipeline::new();
        let src = Self::make_src(capturable, true)?;
        let appsink = Self::make_appsink()?;
        let sink = appsink.clone().upcast::<gst::Element>();
        pipeline.add_many([&src, &sink])?;
        src.link(&sink)
            .map_err(|e| GStreamerError(format!("Failed to link pipewiresrc -> appsink: {e}")))?;
        Ok((pipeline, appsink))
    }

    /// GL DMA-BUF import path for compositors that only offer tiled/modified DMA-BUF.
    ///
    /// glupload imports the dmabuf as an EGLImage (fd + fourcc + modifier), the GPU does
    /// the de-tiling / colour convert, `gldownload` copies it back to system memory and
    /// `videoconvert` guarantees the `BGRx`/`RGBx` the appsink wants.
    ///
    /// The capsfilter restricts negotiation to single-plane, non-CCS modifiers
    /// (LINEAR + X_TILED + Y_TILED). niri defaults to a *CCS* modifier
    /// (Y_TILED_GEN12_MC_CCS = 0x0100000000000008); CCS is multi-plane (colour buffer +
    /// a control-surface plane) but the screen-cast stream carries only one plane, so
    /// `eglCreateImage` rejects it with `EGL_BAD_MATCH` and glupload falls through to a
    /// CPU path that cannot map the buffer -> black screen. A bare fourcc (no
    /// `:modifier`) means LINEAR and doubles as the portable fallback for non-Intel GPUs.
    /// `always-copy` MUST be false: niri sets chunk->size = maxsize = 1 on its dmabufs
    /// (the data lives in the fd), so a copy would keep a 1-byte system buffer and drop
    /// the fd the GL importer needs.
    ///
    /// NB: a *list* of alternative drm-formats must use `{ }` (GstValueList). `< >` builds
    /// a GstValueArray (an ordered fixed tuple) that does NOT intersect the list-valued
    /// `drm-format` sink caps downstream, so the link fails silently.
    fn build_gl(
        capturable: &PipeWireCapturable,
    ) -> Result<(gst::Pipeline, AppSink), Box<dyn Error>> {
        if gst::ElementFactory::find("glupload").is_none()
            || gst::ElementFactory::find("gldownload").is_none()
        {
            return Err(Box::new(GStreamerError(
                "GL DMA-BUF import needed for this compositor but glupload/gldownload are \
                 unavailable (install gstreamer gl plugins)."
                    .into(),
            )));
        }

        let pipeline = gst::Pipeline::new();
        let src = Self::make_src(capturable, false)?;

        let dmabuf_caps: gst::Caps = "video/x-raw(memory:DMABuf), format=(string)DMA_DRM, \
             width=(int)[1,32767], height=(int)[1,32767], \
             framerate=(fraction)[0/1,2147483647/1], drm-format=(list){ \
             (string)XR24, (string)XR24:0x0100000000000001, (string)XR24:0x0100000000000002, \
             (string)AR24, (string)AR24:0x0100000000000001, (string)AR24:0x0100000000000002 }"
            .parse()
            .map_err(|e| GStreamerError(format!("Failed to parse DMA-BUF caps: {e}")))?;
        let capsfilter = gst::ElementFactory::make("capsfilter").build()?;
        capsfilter.set_property("caps", &dmabuf_caps);

        let glupload = gst::ElementFactory::make("glupload").build()?;
        let glcolorconvert = gst::ElementFactory::make("glcolorconvert").build()?;
        let gldownload = gst::ElementFactory::make("gldownload").build()?;
        let videoconvert = gst::ElementFactory::make("videoconvert").build()?;
        let appsink = Self::make_appsink()?;
        let sink = appsink.clone().upcast::<gst::Element>();

        let elements = [
            src,
            capsfilter,
            glupload,
            glcolorconvert,
            gldownload,
            videoconvert,
            sink,
        ];
        pipeline.add_many(&elements)?;
        for w in elements.windows(2) {
            w[0].link(&w[1]).map_err(|e| {
                GStreamerError(format!("Failed to link {} -> {}: {e}", w[0].name(), w[1].name()))
            })?;
        }
        Ok((pipeline, appsink))
    }

    /// Zero-copy path: keep the PipeWire buffer as a DMA_DRM dmabuf and hand its fd
    /// downstream (to `lib/encode_video.c`'s hwmap import). No GPU download, no colour
    /// convert — the VAAPI encoder consumes the tiled buffer directly.
    ///
    /// `always-copy` MUST be false (same reason as `build_gl`): niri stores the data in
    /// the fd with a 1-byte system chunk, so a copy would drop the fd we need. The
    /// capsfilter restricts to single-plane non-CCS modifiers `XR24_NONCCS`.
    fn build_dmabuf(
        capturable: &PipeWireCapturable,
    ) -> Result<(gst::Pipeline, AppSink), Box<dyn Error>> {
        let pipeline = gst::Pipeline::new();
        let src = Self::make_src(capturable, false)?;
        let capsfilter = gst::ElementFactory::make("capsfilter").build()?;
        capsfilter.set_property(
            "caps",
            &XR24_NONCCS
                .parse::<gst::Caps>()
                .map_err(|e| GStreamerError(format!("Failed to parse dmabuf caps: {e}")))?,
        );
        let appsink = gst::ElementFactory::make("appsink").build()?;
        appsink.set_property("drop", &true);
        appsink.set_property("max-buffers", &1u32);
        let appsink = appsink
            .dynamic_cast::<AppSink>()
            .map_err(|_| GStreamerError("Sink element is expected to be an appsink!".into()))?;
        // Keep the buffer as a DMA_DRM dmabuf — do NOT download to system memory.
        appsink.set_caps(Some(
            &"video/x-raw(memory:DMABuf), format=(string)DMA_DRM"
                .parse::<gst::Caps>()
                .map_err(|e| GStreamerError(format!("Failed to parse appsink caps: {e}")))?,
        ));
        let sink = appsink.clone().upcast::<gst::Element>();
        pipeline.add_many([&src, &capsfilter, &sink])?;
        src.link(&capsfilter)
            .map_err(|e| GStreamerError(format!("Failed to link pipewiresrc -> capsfilter: {e}")))?;
        capsfilter
            .link(&sink)
            .map_err(|e| GStreamerError(format!("Failed to link capsfilter -> appsink: {e}")))?;
        Ok((pipeline, appsink))
    }

    /// Cheap-readback CPU path: identical to `build_gl` but the capsfilter forces a LINEAR
    /// modifier (`XR24_LINEAR`), so `gldownload` copies untiled pixels back to system
    /// memory with no de-tile cost. Yields the same `BGRx`/`RGBx` any encoder accepts.
    fn build_linear_cpu(
        capturable: &PipeWireCapturable,
    ) -> Result<(gst::Pipeline, AppSink), Box<dyn Error>> {
        if gst::ElementFactory::find("glupload").is_none()
            || gst::ElementFactory::find("gldownload").is_none()
        {
            return Err(Box::new(GStreamerError(
                "GL DMA-BUF import needed for the linear-cpu path but glupload/gldownload are \
                 unavailable (install gstreamer gl plugins)."
                    .into(),
            )));
        }

        let pipeline = gst::Pipeline::new();
        let src = Self::make_src(capturable, false)?;

        let dmabuf_caps: gst::Caps = XR24_LINEAR
            .parse()
            .map_err(|e| GStreamerError(format!("Failed to parse LINEAR DMA-BUF caps: {e}")))?;
        let capsfilter = gst::ElementFactory::make("capsfilter").build()?;
        capsfilter.set_property("caps", &dmabuf_caps);

        let glupload = gst::ElementFactory::make("glupload").build()?;
        let glcolorconvert = gst::ElementFactory::make("glcolorconvert").build()?;
        let gldownload = gst::ElementFactory::make("gldownload").build()?;
        let videoconvert = gst::ElementFactory::make("videoconvert").build()?;
        let appsink = Self::make_appsink()?;
        let sink = appsink.clone().upcast::<gst::Element>();

        let elements = [
            src,
            capsfilter,
            glupload,
            glcolorconvert,
            gldownload,
            videoconvert,
            sink,
        ];
        pipeline.add_many(&elements)?;
        for w in elements.windows(2) {
            w[0].link(&w[1]).map_err(|e| {
                GStreamerError(format!("Failed to link {} -> {}: {e}", w[0].name(), w[1].name()))
            })?;
        }
        Ok((pipeline, appsink))
    }

    /// Drive a pipeline to PLAYING, returning `true` if it actually got there. Used to
    /// probe the direct path in `auto` mode: a compositor that only offers DMA-BUF makes
    /// negotiation fail (`not-negotiated`) and the state change reports failure quickly.
    fn try_reach_playing(pipeline: &gst::Pipeline, timeout_secs: u64) -> bool {
        match pipeline.set_state(gst::State::Playing) {
            Err(_) => false,
            Ok(gst::StateChangeSuccess::Success) | Ok(gst::StateChangeSuccess::NoPreroll) => true,
            Ok(gst::StateChangeSuccess::Async) => pipeline
                .state(gst::ClockTime::from_seconds(timeout_secs))
                .0
                .is_ok(),
        }
    }

    /// Collect ERROR messages from the pipeline bus. GStreamer's own GST_DEBUG output does
    /// not reach stderr from weylus, so the bus is the only place the real
    /// caps-negotiation / import reason (e.g. pipewiresrc "not-negotiated" or "Internal
    /// data stream error") is available.
    fn drain_bus_errors(pipeline: &gst::Pipeline) -> String {
        let bus = match pipeline.bus() {
            Some(b) => b,
            None => return "no bus available".into(),
        };
        let mut msgs = Vec::new();
        while let Some(msg) = bus
            .timed_pop_filtered(gst::ClockTime::from_mseconds(500), &[gst::MessageType::Error])
        {
            if let gst::MessageView::Error(err) = msg.view() {
                let src = err
                    .src()
                    .map(|s| s.path_string().to_string())
                    .unwrap_or_else(|| "?".into());
                msgs.push(format!(
                    "[{src}] {} (debug: {})",
                    err.error(),
                    err.debug().unwrap_or_else(|| "none".into())
                ));
            }
        }
        if msgs.is_empty() {
            "no error message on bus".into()
        } else {
            msgs.join("; ")
        }
    }

    /// Start a pre-built pipeline, surfacing the real GStreamer error if it cannot reach
    /// PLAYING (instead of the generic "Element failed to change its state!").
    fn start(built: (gst::Pipeline, AppSink)) -> Result<Self, Box<dyn Error>> {
        let (pipeline, appsink) = built;
        if Self::try_reach_playing(&pipeline, 5) {
            Ok(Self::finish(pipeline, appsink))
        } else {
            let reason = Self::drain_bus_errors(&pipeline);
            let _ = pipeline.set_state(gst::State::Null);
            Err(Box::new(GStreamerError(format!(
                "Failed to start pipewire pipeline: {reason}"
            ))))
        }
    }

    fn finish(pipeline: gst::Pipeline, appsink: AppSink) -> Self {
        Self {
            pipeline,
            appsink,
            buffer: None,
            pix_fmt: "".into(),
            width: 0,
            height: 0,
            buffer_cropped: vec![],
            is_cropped: false,
            is_dmabuf: false,
            dmabuf: None,
        }
    }
}

impl Recorder for PipeWireRecorder {
    fn is_dmabuf(&self) -> bool {
        self.is_dmabuf
    }

    fn capture(&mut self) -> Result<PixelProvider<'_>, Box<dyn Error>> {
        // Zero-copy path: pull a DMA_DRM dmabuf, retain the buffer (keeps the fd valid)
        // and hand its DRM layout downstream. Reuse the previous frame if none is ready.
        if self.is_dmabuf {
            if let Some(sample) = self
                .appsink
                .try_pull_sample(gst::ClockTime::from_mseconds(16))
            {
                let caps = sample
                    .caps()
                    .ok_or_else(|| GStreamerError("dmabuf sample has no caps".into()))?;
                let s = caps
                    .structure(0)
                    .ok_or_else(|| GStreamerError("dmabuf caps has no structure".into()))?;
                let w: i32 = s.value("width")?.get()?;
                let h: i32 = s.value("height")?.get()?;
                // The negotiated drm-format carries the ACTUAL modifier the compositor
                // chose (LINEAR / X_TILED / Y_TILED). It must reach the DRM-PRIME
                // descriptor or a tiled buffer is imported as linear -> scrambled frame.
                let drm_format = s
                    .value("drm-format")
                    .ok()
                    .and_then(|v| v.get::<String>().ok())
                    .unwrap_or_default();
                let modifier = parse_drm_modifier(&drm_format);
                let buffer = sample
                    .buffer_owned()
                    .ok_or_else(|| GStreamerError("dmabuf sample has no buffer".into()))?;
                let vmeta = buffer.meta::<gstreamer_video::VideoMeta>();
                let stride = vmeta.as_ref().map(|m| m.stride()[0]).unwrap_or(w * 4) as u32;
                let offset = vmeta.as_ref().map(|m| m.offset()[0]).unwrap_or(0) as u32;
                let fd = buffer
                    .peek_memory(0)
                    .downcast_memory_ref::<gstreamer_allocators::DmaBufMemory>()
                    .map(|m| m.fd())
                    .ok_or_else(|| GStreamerError("buffer memory is not a DmaBufMemory".into()))?;
                // Log the negotiated layout once (or whenever it changes) for diagnosis.
                let changed = self
                    .dmabuf
                    .as_ref()
                    .map(|d| d.6 != modifier || d.3 != stride || d.1 != w as usize)
                    .unwrap_or(true);
                if changed {
                    debug!(
                        "dmabuf frame: {}x{} drm-format={:?} modifier={:#018x} stride={} offset={}",
                        w, h, drm_format, modifier, stride, offset
                    );
                }
                self.dmabuf = Some((buffer, w as usize, h as usize, stride, offset, fd, modifier));
            }
            let (_, w, h, stride, offset, fd, modifier) = self
                .dmabuf
                .as_ref()
                .ok_or_else(|| GStreamerError("No dmabuf frame available!".into()))?;
            return Ok(PixelProvider::DmaBuf {
                fd: *fd,
                // DRM_FORMAT_XRGB8888 = 'X','R','2','4' little-endian.
                fourcc: u32::from_le_bytes([b'X', b'R', b'2', b'4']),
                modifier: *modifier,
                width: *w,
                height: *h,
                stride: *stride,
                offset: *offset,
            });
        }
        if let Some(sample) = self
            .appsink
            .try_pull_sample(gst::ClockTime::from_mseconds(16))
        {
            let cap = sample.caps().unwrap().structure(0).unwrap();
            let w: i32 = cap.value("width")?.get()?;
            let h: i32 = cap.value("height")?.get()?;
            self.pix_fmt = cap.value("format")?.get()?;
            let w = w as usize;
            let h = h as usize;
            let buf = sample
                .buffer_owned()
                .ok_or_else(|| GStreamerError("Failed to get owned buffer.".into()))?;
            let mut crop = buf
                .meta::<gstreamer_video::VideoCropMeta>()
                .map(|m| m.rect());
            // only crop if necessary
            if Some((0, 0, w as u32, h as u32)) == crop {
                crop = None;
            }
            let buf = buf
                .into_mapped_buffer_readable()
                .map_err(|_| GStreamerError("Failed to map buffer.".into()))?;
            let buf_size = buf.size();
            // BGRx is 4 bytes per pixel
            if buf_size != (w * h * 4) {
                // for some reason the width and height of the caps do not guarantee correct buffer
                // size, so ignore those buffers, see:
                // https://gitlab.freedesktop.org/pipewire/pipewire/-/issues/985
                trace!(
                    "Size of mapped buffer: {} does NOT match size of capturable {}x{}@BGRx, \
                    dropping it!",
                    buf_size,
                    w,
                    h
                );
            } else {
                // Copy region specified by crop into self.buffer_cropped
                // TODO: Figure out if ffmpeg provides a zero copy alternative
                if let Some((x_off, y_off, w_crop, h_crop)) = crop {
                    let x_off = x_off as usize;
                    let y_off = y_off as usize;
                    let w_crop = w_crop as usize;
                    let h_crop = h_crop as usize;
                    self.buffer_cropped.clear();
                    let data = buf.as_slice();
                    // BGRx is 4 bytes per pixel
                    self.buffer_cropped.reserve(w_crop * h_crop * 4);
                    for y in y_off..(y_off + h_crop) {
                        let i = 4 * (w * y + x_off);
                        self.buffer_cropped.extend(&data[i..i + 4 * w_crop]);
                    }
                    self.width = w_crop;
                    self.height = h_crop;
                } else {
                    self.width = w;
                    self.height = h;
                }
                self.is_cropped = crop.is_some();
                self.buffer = Some(buf);
            }
        } else {
            trace!("No new buffer available, falling back to previous one.");
        }
        if self.buffer.is_none() {
            return Err(Box::new(GStreamerError("No buffer available!".into())));
        }
        let buf = if self.is_cropped {
            self.buffer_cropped.as_slice()
        } else {
            self.buffer.as_ref().unwrap().as_slice()
        };
        match self.pix_fmt.as_str() {
            "BGRx" => Ok(PixelProvider::BGR0(self.width, self.height, buf)),
            "RGBx" => Ok(PixelProvider::RGB0(self.width, self.height, buf)),
            _ => unreachable!(),
        }
    }
}

impl Drop for PipeWireRecorder {
    fn drop(&mut self) {
        if let Err(err) = self.pipeline.set_state(gst::State::Null) {
            warn!("Failed to stop GStreamer pipeline: {}.", err);
        }
    }
}

fn handle_response<F>(
    portal: Proxy<&SyncConnection>,
    path: dbus::Path<'static>,
    context: Arc<Mutex<CallBackContext>>,
    mut f: F,
) -> Result<dbus::channel::Token, dbus::Error>
where
    F: FnMut(
            OrgFreedesktopPortalRequestResponse,
            Proxy<&SyncConnection>,
            &Message,
            Arc<Mutex<CallBackContext>>,
        ) -> Result<(), Box<dyn Error>>
        + Send
        + Sync
        + 'static,
{
    let mut m = MatchRule::new();
    m.path = Some(path);
    m.msg_type = Some(MessageType::Signal);
    m.sender = Some("org.freedesktop.portal.Desktop".into());
    m.interface = Some("org.freedesktop.portal.Request".into());
    portal
        .connection
        .add_match(m, move |r: OrgFreedesktopPortalRequestResponse, c, m| {
            let portal = get_portal(c);
            debug!("Response from DBus: response: {:?}, message: {:?}", r, m);
            match r.response {
                0 => {}
                1 => {
                    context.lock().unwrap().failure = true;
                    warn!("DBus response: User cancelled interaction.");
                    return true;
                }
                c => {
                    context.lock().unwrap().failure = true;
                    warn!("DBus response: Unknown error, code: {}.", c);
                    return true;
                }
            }
            if let Err(err) = f(r, portal, m, context.clone()) {
                context.lock().unwrap().failure = true;
                warn!("Error requesting screen capture via dbus: {}", err);
            }
            true
        })
}

fn get_portal(conn: &SyncConnection) -> Proxy<'_, &SyncConnection> {
    conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(1000),
    )
}

fn streams_from_response(response: &OrgFreedesktopPortalRequestResponse) -> Vec<PwStreamInfo> {
    (move || {
        Some(
            response
                .results
                .get("streams")?
                .as_iter()?
                .next()?
                .as_iter()?
                .filter_map(|stream| {
                    let mut itr = stream.as_iter()?;
                    let path = itr.next()?.as_u64()?;
                    let (keys, values): (Vec<(usize, &dyn RefArg)>, Vec<(usize, &dyn RefArg)>) =
                        itr.next()?
                            .as_iter()?
                            .enumerate()
                            .partition(|(i, _)| i % 2 == 0);
                    let attributes = keys
                        .iter()
                        .filter_map(|(_, key)| Some(key.as_str()?.to_owned()))
                        .zip(
                            values
                                .iter()
                                .map(|(_, arg)| *arg)
                                .collect::<Vec<&dyn RefArg>>(),
                        )
                        .collect::<HashMap<String, &dyn RefArg>>();
                    Some(PwStreamInfo {
                        path,
                        source_type: attributes
                            .get("source_type")
                            .map_or(Some(0), |v| v.as_u64())?,
                        position: attributes.get("position").and_then(|v| extract_ii(v)),
                        size: attributes.get("size").and_then(|v| extract_ii(v)),
                    })
                })
                .collect::<Vec<PwStreamInfo>>(),
        )
    })()
    .unwrap_or_default()
}

// mostly inspired by https://gitlab.gnome.org/snippets/19 and
// https://gitlab.gnome.org/-/snippets/39
struct CallBackContext {
    capture_cursor: bool,
    session: dbus::Path<'static>,
    streams: Vec<PwStreamInfo>,
    fd: Option<OwnedFd>,
    restore_token: Option<String>,
    has_remote_desktop: bool,
    failure: bool,
}

fn on_create_session_response(
    r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("on_create_session_response");
    let session: dbus::Path = r
        .results
        .get("session_handle")
        .ok_or_else(|| {
            DBusError(format!(
                "Failed to obtain session_handle from response: {:?}",
                r
            ))
        })?
        .as_str()
        .ok_or_else(|| DBusError("Failed to convert session_handle to string.".into()))?
        .to_string()
        .into();

    context.lock().unwrap().session = session.clone();
    if context.lock().unwrap().has_remote_desktop {
        select_devices(portal, context)
    } else {
        select_sources(portal, context)
    }
}

fn select_devices(
    portal: Proxy<&SyncConnection>,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    let mut args: PropMap = HashMap::new();
    let t: usize = rand::random();
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t}"))),
    );

    // TODO
    //args.insert(
    //    "restore_token".to_string(),
    //    Variant(Box::new(format!("weylus{t}"))),
    //);

    // persist modes:
    // 0: Do not persist (default)
    // 1: Permissions persist as long as the application is running
    // 2: Permissions persist until explicitly revoked
    args.insert("persist_mode".to_string(), Variant(Box::new(2 as u32)));

    // device types
    // 1: KEYBOARD
    // 2: POINTER
    // 4: TOUCHSCREEN
    let device_types = portal.available_device_types()?;
    debug!("Available device types: {device_types}.");
    args.insert("types".to_string(), Variant(Box::new(device_types)));

    let path = portal.select_devices(context.lock().unwrap().session.clone(), args)?;
    handle_response(portal, path, context, |_, portal, _, context| {
        select_sources(portal, context)
    })?;
    Ok(())
}

fn select_sources(
    portal: Proxy<&SyncConnection>,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("select_sources");
    let mut args: PropMap = HashMap::new();

    let t: usize = rand::random();
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t}"))),
    );
    // https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html#org-freedesktop-portal-screencast-selectsources
    // allow multiple sources
    args.insert("multiple".into(), Variant(Box::new(true)));

    // 1: MONITOR
    // 2: WINDOW
    // 4: VIRTUAL
    let source_types = portal.available_source_types()?;
    debug!("Available source types: {source_types}.");
    args.insert("types".into(), Variant(Box::new(source_types)));

    let capture_cursor = context.lock().unwrap().capture_cursor;
    // 1: Hidden. The cursor is not part of the screen cast stream.
    // 2: Embedded: The cursor is embedded as part of the stream buffers.
    // 4: Metadata: The cursor is not part of the screen cast stream, but sent as PipeWire stream metadata.
    let cursor_mode = if capture_cursor { 2u32 } else { 1u32 };

    let is_plasma = std::env::var("DESKTOP_SESSION").map_or(false, |s| s.contains("plasma"));
    if is_plasma && capture_cursor {
        // Warn the user if capturing the cursor is tried on kde as this can crash
        // kwin_wayland and tear down the plasma desktop, see:
        // https://bugs.kde.org/show_bug.cgi?id=435042
        warn!(
            "You are attempting to capture the cursor under KDE Plasma, this may crash your \
                    desktop, see https://bugs.kde.org/show_bug.cgi?id=435042 for details! \
                    You have been warned."
        );
    }
    args.insert("cursor_mode".into(), Variant(Box::new(cursor_mode)));

    let path = portal.select_sources(context.lock().unwrap().session.clone(), args)?;
    handle_response(portal, path, context, on_select_sources_response)?;
    Ok(())
}

fn on_select_sources_response(
    _r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("on_select_sources_response");
    let mut args: PropMap = HashMap::new();
    let t: usize = rand::random();
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t}"))),
    );
    let path = if context.lock().unwrap().has_remote_desktop {
        OrgFreedesktopPortalRemoteDesktop::start(
            &portal,
            context.lock().unwrap().session.clone(),
            "",
            args,
        )?
    } else {
        OrgFreedesktopPortalScreenCast::start(
            &portal,
            context.lock().unwrap().session.clone(),
            "",
            args,
        )?
    };
    handle_response(portal, path, context, on_start_response)?;
    Ok(())
}

fn on_start_response(
    r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("on_start_response");
    let mut context = context.lock().unwrap();
    let mut new_streams = streams_from_response(&r);
    for s in &new_streams {
        let kind = match s.source_type {
            1 => "monitor",
            2 => "window",
            _ => "unknown",
        };
        debug!(
            "Portal stream: node {}, source_type {} ({kind}), position {:?}, size {:?}",
            s.path, s.source_type, s.position, s.size
        );
    }
    context.streams.append(&mut new_streams);
    let session = context.session.clone();
    context
        .fd
        .replace(portal.open_pipe_wire_remote(session.clone(), HashMap::new())?);
    if let Some(Some(t)) = r.results.get("restore_token").map(|t| t.as_str()) {
        context.restore_token = Some(t.to_string());
    }
    if context.has_remote_desktop {
        debug!("Remote Desktop Session started");
    } else {
        debug!("Screen Cast Session started");
    }
    Ok(())
}

fn request_remote_desktop(
    capture_cursor: bool,
) -> Result<(SyncConnection, OwnedFd, Vec<PwStreamInfo>), Box<dyn Error>> {
    let conn = SyncConnection::new_session()?;
    let portal = get_portal(&conn);

    // Disabled for KDE plasma due to https://bugs.kde.org/show_bug.cgi?id=484996
    // List of supported DEs: https://wiki.archlinux.org/title/XDG_Desktop_Portal#List_of_backends_and_interfaces
    let has_remote_desktop =
        std::env::var("DESKTOP_SESSION").map_or(false, |s| s.contains("gnome"));

    let context = CallBackContext {
        capture_cursor,
        session: Default::default(),
        streams: Default::default(),
        fd: None,
        restore_token: None,
        has_remote_desktop,
        failure: false,
    };
    let context = Arc::new(Mutex::new(context));

    let mut args: PropMap = HashMap::new();
    let t1: usize = rand::random();
    let t2: usize = rand::random();
    args.insert(
        "session_handle_token".to_string(),
        Variant(Box::new(format!("weylus{t1}"))),
    );
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t2}"))),
    );
    let path = if has_remote_desktop {
        OrgFreedesktopPortalRemoteDesktop::create_session(&portal, args)?
    } else {
        OrgFreedesktopPortalScreenCast::create_session(&portal, args)?
    };
    handle_response(portal, path, context.clone(), on_create_session_response)?;

    // wait 3 minutes for user interaction
    for _ in 0..1800 {
        conn.process(Duration::from_millis(100))?;
        let context = context.lock().unwrap();
        // Once we got a file descriptor we are done!
        if context.fd.is_some() {
            break;
        }

        if context.failure {
            break;
        }
    }
    let context = context.lock().unwrap();
    if context.fd.is_some() && !context.streams.is_empty() {
        Ok((conn, context.fd.clone().unwrap(), context.streams.clone()))
    } else {
        Err(Box::new(DBusError(
            "Failed to obtain screen capture.".into(),
        )))
    }
}

pub fn get_capturables(
    capture_cursor: bool,
    pipeline: PipewirePipeline,
) -> Result<Vec<PipeWireCapturable>, Box<dyn Error>> {
    let (conn, fd, streams) = request_remote_desktop(capture_cursor)?;
    let conn = Arc::new(conn);
    // Query the global output bounding box once so each capturable can map its
    // portal-reported rect into the compositor's logical coordinate space. If this
    // fails (no Wayland display / no xdg-output) geometry() falls back to whole-screen.
    let global_box = crate::capturable::wayland_outputs::global_bounding_box();
    debug!("Wayland global bounding box for stylus mapping: {global_box:?}");
    Ok(streams
        .into_iter()
        .map(|s| PipeWireCapturable::new(conn.clone(), fd.clone(), s, global_box, pipeline))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drm_modifier_bare_fourcc_is_linear() {
        assert_eq!(parse_drm_modifier("XR24"), 0);
    }

    #[test]
    fn drm_modifier_parses_tiled_suffix() {
        // Y_TILED and X_TILED as niri/Intel negotiate them.
        assert_eq!(parse_drm_modifier("XR24:0x0100000000000002"), 0x0100000000000002);
        assert_eq!(parse_drm_modifier("XR24:0x0100000000000001"), 0x0100000000000001);
    }

    #[test]
    fn drm_modifier_explicit_linear_suffix() {
        assert_eq!(parse_drm_modifier("XR24:0x0000000000000000"), 0);
    }

    #[test]
    fn drm_modifier_garbage_falls_back_to_linear() {
        assert_eq!(parse_drm_modifier("XR24:nonsense"), 0);
    }

    fn rel(g: &Geometry) -> (f64, f64, f64, f64) {
        match g {
            Geometry::Relative(x, y, w, h) => (*x, *y, *w, *h),
            #[allow(unreachable_patterns)]
            _ => panic!("expected Relative"),
        }
    }

    const BOX_2688: GlobalBox = GlobalBox {
        x: 0,
        y: 0,
        width: 4096,
        height: 2688,
    };

    #[test]
    fn monitor_maps_into_global_box() {
        // eDP-1 as measured: logical (1328,1728) 1440x960 within the 4096x2688 box.
        let g = relative_geometry(1, Some((1328, 1728)), Some((1440, 960)), Some(BOX_2688));
        let (x, y, w, h) = rel(&g);
        assert!((x - 1328.0 / 4096.0).abs() < 1e-9);
        assert!((y - 1728.0 / 2688.0).abs() < 1e-9);
        assert!((w - 1440.0 / 4096.0).abs() < 1e-9);
        assert!((h - 960.0 / 2688.0).abs() < 1e-9);
    }

    #[test]
    fn primary_monitor_maps_to_origin() {
        // DP-1: logical (0,0) 4096x1728 -> covers full width, top portion.
        let g = relative_geometry(1, Some((0, 0)), Some((4096, 1728)), Some(BOX_2688));
        let (x, y, w, h) = rel(&g);
        assert_eq!((x, y), (0.0, 0.0));
        assert!((w - 1.0).abs() < 1e-9);
        assert!((h - 1728.0 / 2688.0).abs() < 1e-9);
    }

    #[test]
    fn window_falls_back_to_whole_screen() {
        // source_type 2 == window; portal reports a dummy 1x1 we must ignore.
        let g = relative_geometry(2, Some((0, 0)), Some((1, 1)), Some(BOX_2688));
        assert_eq!(rel(&g), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn no_global_box_falls_back_to_whole_screen() {
        let g = relative_geometry(1, Some((1328, 1728)), Some((1440, 960)), None);
        assert_eq!(rel(&g), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn missing_position_falls_back_to_whole_screen() {
        let g = relative_geometry(1, None, Some((1440, 960)), Some(BOX_2688));
        assert_eq!(rel(&g), (0.0, 0.0, 1.0, 1.0));
    }
}
