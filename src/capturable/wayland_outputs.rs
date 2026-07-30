//! Wayland output geometry, used to map stylus input onto the right monitor.
//!
//! The ScreenCast portal hands weylus each captured monitor's rect (`position`/`size`)
//! in the compositor's LOGICAL coordinate space. But the compositor scales the tablet's
//! absolute axes against the *global* bounding rectangle (the union of all outputs), not
//! against a single output. So to turn a portal rect into the `Geometry::Relative`
//! fraction weylus needs, we must divide it by that global box.
//!
//! The portal does not report the global box, so we enumerate outputs ourselves via
//! `wl_output` + the `xdg-output` extension (which reports each output's logical
//! position/size directly). This is compositor-agnostic — no `niri msg`, no reliance on
//! which outputs the portal happened to return.
//!
//! This whole module is Linux-only (same `cfg(target_os = "linux")` gate as the rest of
//! the PipeWire/Wayland path) and is only exercised when the Wayland capture checkbox is
//! enabled. Any failure (no Wayland display, no `xdg-output`) returns `None`, and the
//! caller falls back to the historic whole-screen mapping.

use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::{delegate_output, delegate_registry, registry_handlers};
use tracing::debug;

/// Global bounding box of all outputs in the compositor's logical coordinate space:
/// `(x, y, width, height)`. This is the space the compositor decodes tablet ABS axes
/// against, so it is what a per-monitor portal rect must be normalised by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

struct AppData {
    registry_state: RegistryState,
    output_state: OutputState,
}

impl OutputHandler for AppData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    // We read the accumulated state after roundtrips, so the callbacks are no-ops.
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
}

delegate_output!(AppData);
delegate_registry!(AppData);

impl ProvidesRegistryState for AppData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

/// Query the global bounding box of all Wayland outputs via `xdg-output`.
///
/// Returns `None` (and logs) if there is no Wayland display, the compositor does not
/// expose `xdg-output`, or no output reports logical geometry — in which case the caller
/// should fall back to the historic whole-screen mapping.
pub fn global_bounding_box() -> Option<GlobalBox> {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            debug!("No Wayland connection for output geometry: {e}");
            return None;
        }
    };
    let (globals, mut event_queue) = match registry_queue_init::<AppData>(&conn) {
        Ok(v) => v,
        Err(e) => {
            debug!("Wayland registry init failed: {e}");
            return None;
        }
    };
    let qh = event_queue.handle();
    let mut app = AppData {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
    };

    // First roundtrip: wl_output globals + their wl_output events.
    // Second: the follow-up xdg_output events carrying logical_position/logical_size.
    if let Err(e) = event_queue.roundtrip(&mut app) {
        debug!("Wayland roundtrip failed: {e}");
        return None;
    }
    if let Err(e) = event_queue.roundtrip(&mut app) {
        debug!("Wayland roundtrip failed: {e}");
        return None;
    }

    let infos: Vec<_> = app
        .output_state
        .outputs()
        .filter_map(|o| app.output_state.info(&o))
        .collect();

    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    for info in &infos {
        let name = info.name.as_deref().unwrap_or("<unnamed>");
        match (info.logical_position, info.logical_size) {
            (Some((x, y)), Some((w, h))) => {
                debug!(
                    "Wayland output {name} (id {}): logical position ({x}, {y}) size {w}x{h}, \
                     scale {}",
                    info.id, info.scale_factor
                );
                rects.push((x, y, w, h));
            }
            _ => debug!(
                "Wayland output {name} (id {}): no xdg-output logical geometry, skipping",
                info.id
            ),
        }
    }

    let bbox = bounding_box(&rects);
    match bbox {
        Some(b) => debug!(
            "Wayland global bounding box (union of {} output(s)): position ({}, {}) size {}x{}",
            rects.len(),
            b.x,
            b.y,
            b.width,
            b.height
        ),
        None => debug!("No Wayland outputs reported logical geometry; global box unavailable"),
    }
    bbox
}

/// Union of logical rects `(x, y, w, h)` into a `GlobalBox`. `None` if empty.
fn bounding_box(rects: &[(i32, i32, i32, i32)]) -> Option<GlobalBox> {
    let mut it = rects.iter();
    let &(x0, y0, w0, h0) = it.next()?;
    let mut min_x = x0;
    let mut min_y = y0;
    let mut max_x = x0 + w0;
    let mut max_y = y0 + h0;
    for &(x, y, w, h) in it {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + w);
        max_y = max_y.max(y + h);
    }
    Some(GlobalBox {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounding_box_of_measured_dual_monitor_setup() {
        // The exact logical rects this box reports (coords.txt): DP-1 + eDP-1.
        let rects = [(0, 0, 4096, 1728), (1328, 1728, 1440, 960)];
        assert_eq!(
            bounding_box(&rects),
            Some(GlobalBox {
                x: 0,
                y: 0,
                width: 4096,
                height: 2688,
            })
        );
    }

    #[test]
    fn bounding_box_handles_negative_origin() {
        // Output to the left of / above the primary => negative coordinates.
        let rects = [(0, 0, 1920, 1080), (-1280, -200, 1280, 1024)];
        assert_eq!(
            bounding_box(&rects),
            Some(GlobalBox {
                x: -1280,
                y: -200,
                width: 1920 + 1280,
                height: 1080 + 200,
            })
        );
    }

    #[test]
    fn bounding_box_empty_is_none() {
        assert_eq!(bounding_box(&[]), None);
    }
}
