use std::fmt::Display;

use serde_json::{Map, Number, Value};
use uuid::Uuid;

use super::*;

#[inline]
fn serialize_style0(style: &Style, map: &mut Map<String, Value>) {
    if let Some(color) = style.color {
        map.insert("color".to_string(), Value::String(match color {
            TextColor::Black => "black".to_string(),
            TextColor::DarkBlue => "dark_blue".to_string(),
            TextColor::DarkGreen => "dark_green".to_string(),
            TextColor::DarkAqua => "dark_aqua".to_string(),
            TextColor::DarkRed => "dark_red".to_string(),
            TextColor::DarkPurple => "dark_purple".to_string(),
            TextColor::Gold => "gold".to_string(),
            TextColor::Gray => "gray".to_string(),
            TextColor::DarkGray => "dark_gray".to_string(),
            TextColor::Blue => "blue".to_string(),
            TextColor::Green => "green".to_string(),
            TextColor::Aqua => "aqua".to_string(),
            TextColor::Red => "red".to_string(),
            TextColor::LightPurple => "light_purple".to_string(),
            TextColor::Yellow => "yellow".to_string(),
            TextColor::White => "white".to_string(),
            TextColor::Hex(color) => format!("#{:06X}", color),
        }));
    }
    if let Some(bold) = style.bold {
        map.insert("bold".to_string(), Value::Bool(bold));
    }
    if let Some(italic) = style.italic {
        map.insert("italic".to_string(), Value::Bool(italic));
    }
    if let Some(underlined) = style.underlined {
        map.insert("underlined".to_string(), Value::Bool(underlined));
    }
    if let Some(strikethrough) = style.strikethrough {
        map.insert("strikethrough".to_string(), Value::Bool(strikethrough));
    }
    if let Some(obfuscated) = style.obfuscated {
        map.insert("obfuscated".to_string(), Value::Bool(obfuscated));
    }
    if let Some(shadow) = style.shadow_color {
        map.insert("shadow_color".to_string(), Value::Number(Number::from(shadow as i32)));
    }
}

#[inline]
fn serialize_content_into(text: &Text, map: &mut Map<String, Value>) {
    match &text.content {
        TextContent::Literal(text) => {
            map.insert("text".to_string(), text.clone().into());
        }
        TextContent::Translation { key, fallback, with } => {
            map.insert("translate".to_string(), key.clone().into());
            if let Some(fallback) = fallback {
                map.insert("fallback".to_string(), fallback.clone().into());
            }
            if let Some(with) = with {
                map.insert("with".to_string(), Value::Array(with.iter().map(serialize_json).collect()));
            }
        }
        TextContent::Score { name, objective, value } => {
            let mut score_map = Map::new();
            score_map.insert("name".to_string(), Value::String(name.clone()));
            score_map.insert("objective".to_string(), Value::String(objective.clone()));
            if let Some(value) = value {
                score_map.insert("value".to_string(), Value::String(value.clone()));
            }
            map.insert("score".to_string(), Value::Object(score_map));
        }
        TextContent::EntitySelector { selector, separator } => {
            map.insert("selector".to_string(), Value::String(selector.clone()));
            if let Some(separator) = separator {
                map.insert("separator".to_string(), serialize_json(separator));
            }
        }
        TextContent::Keybind(key) => {
            map.insert("keybind".to_string(), Value::String(key.clone()));
        }
        TextContent::Nbt { nbt, interpret, separator, block, entity, storage } => {
            map.insert("nbt".to_string(), Value::String(nbt.clone()));
            if let Some(interpret) = interpret {
                map.insert("interpret".to_string(), Value::Bool(*interpret));
            }
            if let Some(separator) = separator {
                map.insert("separator".to_string(), serialize_json(separator));
            }
            if let Some(block) = block {
                map.insert("block".to_string(), Value::String(block.clone()));
            }
            if let Some(entity) = entity {
                map.insert("entity".to_string(), Value::String(entity.clone()));
            }
            if let Some(storage) = storage {
                map.insert("storage".to_string(), Value::String(storage.clone()));
            }
        }
    }
}

#[inline]
pub fn serialize_style(style: &Style) -> Value {
    let mut map = Map::new();
    serialize_style0(style, &mut map);
    Value::Object(map)
}

#[inline]
pub fn serialize_json(text: &Text) -> Value {
    if let TextContent::Literal(str) = &text.content {
        if text.style.is_empty() && text.insertion.is_none()
            && text.click_event.is_none() && text.hover_event.is_none() && text.extra.is_empty() {
            return Value::String(str.clone());
        }
    }

    let mut map = Map::new();
    serialize_style0(&text.style, &mut map);

    if let Some(ref event) = text.click_event {
        map.insert("clickEvent".to_string(), serde_json::json!({
            "action": match event.action {
                ClickAction::OpenUrl => "open_url",
                ClickAction::OpenFile => "open_file",
                ClickAction::RunCommand => "run_command",
                ClickAction::SuggestCommand => "suggest_command",
                ClickAction::ChangePage => "change_page",
                ClickAction::CopyToClipboard => "copy_to_clipboard",
                ClickAction::Unresolved(ref action) => action,
            }.to_string(),
            "value": event.value,
        }));
    }
    if let Some(ref event) = text.hover_event {
        map.insert("hoverEvent".to_string(), match event {
            HoverEvent::ShowText(text) => serde_json::json!({
                "action": "show_text",
                "contents": serialize_json(text),
            }),
            HoverEvent::ShowItem { id, count, tag } => {
                let mut map = Map::new();
                map.insert("id".to_string(), Value::String(id.clone()));
                if let Some(count) = count {
                    map.insert("count".to_string(), Value::Number(Number::from(*count)));
                }
                if let Some(tag) = tag {
                    map.insert("tag".to_string(), Value::String(tag.clone()));
                }
                serde_json::json!({
                    "action": "show_item",
                    "contents": map,
                })
            }
            HoverEvent::ShowEntity { id, name, entity_type } => {
                let mut map = Map::new();
                map.insert("id".to_string(), Value::String(id.clone()));
                map.insert("type".to_string(), Value::String(entity_type.clone()));
                if let Some(name) = name {
                    map.insert("name".to_string(), Value::String(name.clone()));
                }
                serde_json::json!({
                    "action": "show_entity",
                    "contents": map,
                })
            }
            HoverEvent::Unresolved { action, value } => serde_json::json!({
                "action": action,
                "value": value,
            }),
        });
    }
    if let Some(ref insertion) = text.insertion {
        map.insert("insertion".to_string(), Value::String(insertion.clone()));
    }
    serialize_content_into(text, &mut map);

    // if !text.extra.is_empty() {
    //     map.insert("extra".to_string(), Value::Array(text.extra.iter().map(serialize_json).collect()));
    // }

    let mut value = Value::Object(map);
    if !text.extra.is_empty() {
        if let Value::Object(map) = &mut value {
            if map.len() == 1 && map.contains_key("text") {
                value = Value::String(match map["text"].take() {
                    Value::String(s) => s,
                    _ => unreachable!(),
                });
            }
        }
        Value::Array((std::iter::once(value).chain(text.extra.iter().map(serialize_json))).collect())
    } else {
        value
    }
}

// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum DeserializationError {
    TextArrayEmpty,
    InvalidValueType,
    InvalidColorFormat,
    MissingClickEventAction,
    MissingClickEventValue,
    MissingHoverEventAction,
    InvalidHoverEventAction,
    MissingHoverEventValue,
    MalformedHoverEventValue,
    MissingScoreName,
    MissingScoreObjective,
    MissingNbtTarget,
    CouldNotIdentifyContentType,
}

impl Display for DeserializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::TextArrayEmpty => write!(f, "Text array is empty"),
            Self::InvalidValueType => write!(f, "Invalid value type"),
            Self::InvalidColorFormat => write!(f, "Invalid color format"),
            Self::MissingClickEventAction => write!(f, "Missing click event action"),
            Self::MissingClickEventValue => write!(f, "Missing click event value"),
            Self::MissingHoverEventAction => write!(f, "Missing hover event action"),
            Self::InvalidHoverEventAction => write!(f, "Invalid hover event action"),
            Self::MissingHoverEventValue => write!(f, "Missing hover event value"),
            Self::MalformedHoverEventValue => write!(f, "Malformed hover event value"),
            Self::MissingScoreName => write!(f, "Missing score name"),
            Self::MissingScoreObjective => write!(f, "Missing score objective"),
            Self::MissingNbtTarget => write!(f, "Missing nbt target"),
            Self::CouldNotIdentifyContentType => write!(f, "Could not identify content type"),
        }
    }
}

impl std::error::Error for DeserializationError {}

type Result<T> = std::result::Result<T, DeserializationError>;

#[inline]
fn deserialize_style0(map: &Map<String, Value>) -> Result<Style> {
    let mut style = Style::empty();
    if let Some(color) = map.get("color").map(|v| v.as_str()).flatten() {
        style.color = Some(match color {
            "black" => TextColor::Black,
            "dark_blue" => TextColor::DarkBlue,
            "dark_green" => TextColor::DarkGreen,
            "dark_aqua" => TextColor::DarkAqua,
            "dark_red" => TextColor::DarkRed,
            "dark_purple" => TextColor::DarkPurple,
            "gold" => TextColor::Gold,
            "gray" => TextColor::Gray,
            "dark_gray" => TextColor::DarkGray,
            "blue" => TextColor::Blue,
            "green" => TextColor::Green,
            "aqua" => TextColor::Aqua,
            "red" => TextColor::Red,
            "light_purple" => TextColor::LightPurple,
            "yellow" => TextColor::Yellow,
            "white" => TextColor::White,
            color => {
                if color.len() == 7 && color.starts_with('#') {
                    let color = u32::from_str_radix(&color[1..], 16).map_err(|_| DeserializationError::InvalidColorFormat)?;
                    TextColor::Hex(color)
                } else {
                    return Err(DeserializationError::InvalidColorFormat);
                }
            }
        });
    }
    style.font = map.get("font").map(|v| v.as_str().map(String::from)).flatten();
    style.shadow_color = map.get("shadow_color").map(|v| v.as_i64().map(|v| v as u32)).flatten();
    style.bold = map.get("bold").map(|v| v.as_bool()).flatten();
    style.italic = map.get("italic").map(|v| v.as_bool()).flatten();
    style.underlined = map.get("underlined").map(|v| v.as_bool()).flatten();
    style.strikethrough = map.get("strikethrough").map(|v| v.as_bool()).flatten();
    style.obfuscated = map.get("obfuscated").map(|v| v.as_bool()).flatten();
    Ok(style)
}

#[inline]
fn deserialize_content(map: &Map<String, Value>) -> Result<TextContent> {
    let text = map.get("text");
    if let Some(text) = text {
        match text {
            Value::String(str) => {
                return Ok(TextContent::Literal(str.clone()));
            }
            Value::Bool(b) => {
                return Ok(TextContent::Literal(b.to_string()));
            }
            Value::Number(n) => {
                return Ok(TextContent::Literal(n.to_string()));
            }
            _ => return Err(DeserializationError::InvalidValueType),
        }
    }
    if let Some(translate) = map.get("translate").map(|v| v.as_str()).flatten().map(String::from) {
        let fallback = map.get("fallback").map(|v| v.as_str()).flatten().map(String::from);
        let with = if let Some(list) = map.get("with").map(|v| v.as_array()).flatten() {
            let mut with = Vec::new();
            for value in list {
                with.push(deserialize_json(value)?);
            }
            Some(with)
        } else {
            None
        };
        return Ok(TextContent::Translation { key: translate, fallback, with });
    }
    if let Some(keybind) = map.get("keybind").map(|v| v.as_str()).flatten().map(String::from).map(TextContent::Keybind) {
        return Ok(keybind);
    }
    if let Some(map) = map.get("score").map(|v| v.as_object()).flatten() {
        let name = map.get("name").map(|v| v.as_str()).flatten().map(String::from).ok_or(DeserializationError::MissingScoreName)?;
        let objective = map.get("objective").map(|v| v.as_str()).flatten().map(String::from).ok_or(DeserializationError::MissingScoreObjective)?;
        let value = map.get("value").map(|v| v.as_str()).flatten().map(String::from);
        return Ok(TextContent::Score { name, objective, value });
    }
    if let Some(selector) = map.get("selector").map(|v| v.as_str()).flatten().map(String::from) {
        let separator = match map.get("separator") {
            Some(s) => Some(Box::new(deserialize_json(s)?)),
            None => None,
        };
        return Ok(TextContent::EntitySelector { selector, separator });
    }
    if let Some(nbt) = map.get("nbt").map(|v| v.as_str()).flatten().map(String::from) {
        let block = map.get("block").map(|v| v.as_str()).flatten().map(String::from);
        let entity = map.get("entity").map(|v| v.as_str()).flatten().map(String::from);
        let storage = map.get("storage").map(|v| v.as_str()).flatten().map(String::from);
        let interpret = map.get("interpret").map(|v| v.as_bool()).flatten();
        let separator = match map.get("separator") {
            Some(s) => Some(Box::new(deserialize_json(s)?)),
            None => None,
        };
        if entity.is_none() && block.is_none() && storage.is_none() {
            return Err(DeserializationError::MissingNbtTarget);
        }
        return Ok(TextContent::Nbt { nbt, interpret, separator, block, entity, storage });
    }
    Err(DeserializationError::CouldNotIdentifyContentType)
}

#[inline]
pub fn deserialize_style(value: &Value) -> Result<Style> {
    match value {
        Value::Object(map) => deserialize_style0(map),
        _ => Err(DeserializationError::InvalidValueType),
    }
}

#[inline]
pub fn deserialize_json(value: &Value) -> Result<Text> {
    match value {
        Value::Array(array) => {
            if array.is_empty() {
                return Err(DeserializationError::TextArrayEmpty);
            }
            let mut text = deserialize_json(&array[0])?;
            if array.len() > 1 {
                for child in array[1..].iter().map(deserialize_json) {
                    text.extra.push(child?);
                }
            }
            Ok(text)
        }
        Value::Object(obj) => {
            let style = deserialize_style0(obj)?;
            let click_event = match obj.get("clickEvent").map(|v| v.as_object()).flatten() {
                Some(map) => {
                    let action = match map.get("action").map(|v| v.as_str()).flatten() {
                        Some(action) => match action {
                            "open_url" => ClickAction::OpenUrl,
                            "open_file" => ClickAction::OpenFile,
                            "run_command" => ClickAction::RunCommand,
                            "suggest_command" => ClickAction::SuggestCommand,
                            "change_page" => ClickAction::ChangePage,
                            "copy_to_clipboard" => ClickAction::CopyToClipboard,
                            action => ClickAction::Unresolved(action.to_string()),
                        },
                        None => return Err(DeserializationError::MissingClickEventAction),
                    };
                    let value = map.get("value").map(|v| v.as_str()).flatten().map(String::from).ok_or(DeserializationError::MissingClickEventValue)?;
                    Some(ClickEvent { action, value })
                }
                None => None,
            };
            let hover_event = match obj.get("hoverEvent").map(|v| v.as_object()).flatten() {
                Some(map) => {
                    Some(match map.get("action").map(|v| v.as_str()).flatten() {
                        Some(action) => {
                            let contents_value = map.get("contents");
                            if let Some(contents) = contents_value.map(|v| v.as_object()).flatten() {
                                let contents_value = contents_value.unwrap();
                                match action {
                                    "show_text" => HoverEvent::ShowText(Box::new(deserialize_json(contents_value)?)),
                                    "show_item" => {
                                        let id = contents.get("id").map(|v| v.as_str()).flatten().map(String::from).ok_or(DeserializationError::MissingHoverEventValue)?;
                                        let count = contents.get("count").map(|v| v.as_i64()).flatten().map(|v| v as i32);
                                        let tag = contents.get("tag").map(|v| v.as_str()).flatten().map(String::from);
                                        HoverEvent::ShowItem { id, count, tag }
                                    }
                                    "show_entity" => {
                                        let name = contents.get("name").map(|v| v.as_str()).flatten().map(String::from);
                                        let entity_type = contents.get("type").map(|v| v.as_str()).flatten().map(String::from).ok_or(DeserializationError::MissingHoverEventValue)?;
                                        let id = match contents.get("id") {
                                            Some(id) => match id {
                                                Value::String(s) => s.clone(),
                                                Value::Array(array) => {
                                                    if array.len() != 4 {
                                                        return Err(DeserializationError::MalformedHoverEventValue);
                                                    }
                                                    if !array.iter().all(|v| v.is_i64()) {
                                                        return Err(DeserializationError::MalformedHoverEventValue);
                                                    }
                                                    let v0 = array[0].as_i64().unwrap() as u32;
                                                    let v1 = array[1].as_i64().unwrap() as u32;
                                                    let v2 = array[2].as_i64().unwrap() as u32;
                                                    let v3 = array[3].as_i64().unwrap() as u32;
                                                    let p0 = (v0 as u64) << 32 | (v1 as u64 & 0xFFFFFFFFu64);
                                                    let p1 = (v2 as u64) << 32 | (v3 as u64 & 0xFFFFFFFFu64);
                                                    Uuid::from_u64_pair(p0, p1).to_string()
                                                }
                                                _ => return Err(DeserializationError::MalformedHoverEventValue),
                                            },
                                            None => return Err(DeserializationError::MissingHoverEventValue),
                                        };
                                        HoverEvent::ShowEntity { id, name, entity_type }
                                    }
                                    _ => return Err(DeserializationError::InvalidHoverEventAction),
                                }
                            } else if let Some(value) = map.get("value").map(|v| v.as_str()).flatten() {
                                HoverEvent::Unresolved { action: action.to_string(), value: value.to_string() }
                            } else {
                                return Err(DeserializationError::MissingHoverEventValue);
                            }
                        }
                        None => return Err(DeserializationError::InvalidValueType),
                    })
                }
                None => None,
            };
            let mut extra = Vec::new();
            if let Some(list) = obj.get("extra").map(|v| v.as_array()).flatten() {
                for value in list {
                    extra.push(deserialize_json(value)?);
                }
            }

            let insertion = obj.get("insertion").map(|v| v.as_str()).flatten().map(String::from);
            let content = deserialize_content(obj)?;
            Ok(Text {
                style,
                click_event,
                hover_event,
                insertion,
                extra,
                content,
            })
        }
        Value::String(str) => {
            Ok(str.clone().into())
        }
        Value::Bool(b) => {
            Ok(b.to_string().into())
        }
        Value::Number(n) => {
            Ok(n.to_string().into())
        }
        _ => Err(DeserializationError::InvalidValueType),
    }
}
