use serde::{Deserialize, Deserializer, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ClientConfiguration {
    #[cfg(target_os = "linux")]
    pub uinput_support: bool,
    pub capturable_id: usize,
    pub capture_cursor: bool,
    pub max_width: usize,
    pub max_height: usize,
    pub client_name: Option<String>,
    pub frame_rate: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MessageInbound {
    PointerEvent(PointerEvent),
    WheelEvent(WheelEvent),
    KeyboardEvent(KeyboardEvent),
    GetCapturableList,
    Config(ClientConfiguration),
    PauseVideo,
    ResumeVideo,
    RestartVideo,
    ChooseCustomInputAreas,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MessageOutbound {
    CapturableList(Vec<String>),
    NewVideo,
    ConfigOk,
    CustomInputAreas(CustomInputAreas),
    ConfigError(String),
    Error(String),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct CustomInputAreas {
    pub mouse: Option<Rect>,
    pub touch: Option<Rect>,
    pub pen: Option<Rect>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum PointerType {
    #[serde(rename = "")]
    Unknown,
    #[serde(rename = "mouse")]
    Mouse,
    #[serde(rename = "pen")]
    Pen,
    #[serde(rename = "touch")]
    Touch,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum PointerEventType {
    #[serde(rename = "pointerdown")]
    DOWN,
    #[serde(rename = "pointerup")]
    UP,
    #[serde(rename = "pointercancel")]
    CANCEL,
    #[serde(rename = "pointermove")]
    MOVE,
    #[serde(rename = "pointerover")]
    OVER,
    #[serde(rename = "pointerenter")]
    ENTER,
    #[serde(rename = "pointerleave")]
    LEAVE,
    #[serde(rename = "pointerout")]
    OUT,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum KeyboardEventType {
    #[serde(rename = "down")]
    DOWN,
    #[serde(rename = "up")]
    UP,
    #[serde(rename = "repeat")]
    REPEAT,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum KeyboardLocation {
    STANDARD,
    LEFT,
    RIGHT,
    NUMPAD,
}

fn location_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<KeyboardLocation, D::Error> {
    let code: u8 = Deserialize::deserialize(deserializer)?;
    match code {
        0 => Ok(KeyboardLocation::STANDARD),
        1 => Ok(KeyboardLocation::LEFT),
        2 => Ok(KeyboardLocation::RIGHT),
        3 => Ok(KeyboardLocation::NUMPAD),
        _ => Err(serde::de::Error::custom(
            "Failed to parse keyboard location code.",
        )),
    }
}

bitflags! {
    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
    pub struct Button: u8 {
        const NONE = 0b0000_0000;
        const PRIMARY = 0b0000_0001;
        const SECONDARY = 0b0000_0010;
        const AUXILARY = 0b0000_0100;
        const FOURTH = 0b0000_1000;
        const FIFTH = 0b0001_0000;
        const ERASER = 0b0010_0000;
    }
}

fn button_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Button, D::Error> {
    let bits: u8 = Deserialize::deserialize(deserializer)?;
    Button::from_bits(bits).map_or(
        Err(serde::de::Error::custom("Failed to parse button code.")),
        Ok,
    )
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KeyboardEvent {
    pub event_type: KeyboardEventType,
    pub code: String,
    pub key: String,
    #[serde(deserialize_with = "location_from")]
    pub location: KeyboardLocation,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PointerEvent {
    pub event_type: PointerEventType,
    pub pointer_id: i64,
    pub timestamp: u64,
    pub is_primary: bool,
    pub pointer_type: PointerType,
    #[serde(deserialize_with = "button_from")]
    pub button: Button,
    #[serde(deserialize_with = "button_from")]
    pub buttons: Button,
    pub x: f64,
    pub y: f64,
    pub movement_x: i64,
    pub movement_y: i64,
    pub pressure: f64,
    pub tilt_x: i32,
    pub tilt_y: i32,
    pub twist: i32,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WheelEvent {
    pub dx: i32,
    pub dy: i32,
    pub timestamp: u64,
}

pub trait WeylusSender {
    type Error: std::error::Error;
    fn send_message(&mut self, message: MessageOutbound) -> Result<(), Self::Error>;
    fn send_video(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

pub trait WeylusReceiver: Iterator<Item = Result<MessageInbound, Self::Error>> {
    type Error: std::error::Error;
}
