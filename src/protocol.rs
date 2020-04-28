use serde::{Deserialize, Serialize, Deserializer};


#[derive(Serialize, Deserialize, Debug)]
pub enum NetMessage {
    PointerEvent(PointerEvent),
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub enum PointerEventType {
    #[serde(rename = "pointerdown")]
    DOWN,
    #[serde(rename = "pointerup")]
    UP,
    #[serde(rename = "pointercancel")]
    CANCEL,
    #[serde(rename = "pointermove")]
    MOVE,
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct Button: u8 {
        const NONE = 0b00000000;
        const PRIMARY = 0b00000001;
        const SECONDARY = 0b00000010;
        const AUXILARY = 0b00000100;
        const FOURTH = 0b00001000;
        const FIFTH = 0b00010000;
    }
}

fn from_str<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Button, D::Error>
{
    let bits: u8 = Deserialize::deserialize(deserializer)?;
    Ok(Button::from_bits_truncate(bits))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PointerEvent {
    pub event_type: PointerEventType,
    pub pointer_id: i64,
    pub is_primary: bool,
    pub pointer_type: PointerType,
    #[serde(deserialize_with = "from_str")]
    pub button: Button,
    #[serde(deserialize_with = "from_str")]
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
