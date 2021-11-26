pub mod autopilot_device;
pub mod device;

#[cfg(target_os = "windows")]
pub mod autopilot_device_win;

#[cfg(target_os = "linux")]
pub mod uinput_device;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub mod uinput_keys;
