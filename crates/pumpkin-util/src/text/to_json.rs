//! A text component as the JSON the Java protocol sends, which changes shape with the client version.

use serde_json::{Map, Value};

use super::click::ClickEvent;
use super::color::Color;
use super::hover::HoverEvent;
use super::style::Style;
use super::{TextComponentBase, TextContent, nbt_compound_to_json};
use crate::translation::Locale;
use crate::version::JavaMinecraftVersion;

impl TextComponentBase {
    /// Converts this component to a `serde_json::Value` for a specific Minecraft version.
    #[must_use]
    pub fn to_json_value_for_version(&self, version: &JavaMinecraftVersion) -> Value {
        if *version >= JavaMinecraftVersion::V_1_20_3
            && self.style.is_empty()
            && self.extra.is_empty()
            && let TextContent::Text { text } = &*self.content
        {
            return Value::String(text.to_string());
        }

        let mut map = Map::new();
        insert_content(&mut map, &self.content, *version);
        insert_style(&mut map, &self.style, *version);
        if !self.extra.is_empty() {
            map.insert(
                "extra".to_string(),
                Value::Array(array(&self.extra, *version)),
            );
        }
        Value::Object(map)
    }
}

fn string<T: ToString + ?Sized>(value: &T) -> Value {
    Value::String(value.to_string())
}

fn array(components: &[TextComponentBase], version: JavaMinecraftVersion) -> Vec<Value> {
    components
        .iter()
        .map(|component| component.to_json_value_for_version(&version))
        .collect()
}

/// One component as itself, several as an array.
fn one_or_array(components: &[TextComponentBase], version: JavaMinecraftVersion) -> Value {
    if components.len() == 1 {
        components[0].to_json_value_for_version(&version)
    } else {
        Value::Array(array(components, version))
    }
}

fn insert_translation(
    map: &mut Map<String, Value>,
    key: &str,
    with: &[TextComponentBase],
    version: JavaMinecraftVersion,
) {
    map.insert("translate".to_string(), string(key));
    if !with.is_empty() {
        map.insert("with".to_string(), Value::Array(array(with, version)));
    }
}

fn insert_content(
    map: &mut Map<String, Value>,
    content: &TextContent,
    version: JavaMinecraftVersion,
) {
    match content {
        TextContent::Text { text } => {
            map.insert("text".to_string(), string(text));
        }
        TextContent::Translate {
            translate, with, ..
        } => insert_translation(map, translate, with, version),
        TextContent::EntityNames {
            selector,
            separator,
        } => {
            map.insert("selector".to_string(), string(selector));
            if let Some(sep) = separator {
                map.insert("separator".to_string(), string(sep));
            }
        }
        TextContent::Keybind { keybind } => {
            map.insert("keybind".to_string(), string(keybind));
        }
        TextContent::Custom { key, with, .. } => insert_translation(map, key, with, version),
        TextContent::PlayerSprite {
            type_name,
            profile,
            hat,
        } => {
            if version >= JavaMinecraftVersion::V_26_1 {
                let full_type = if type_name.contains(':') {
                    type_name.to_string()
                } else {
                    format!("minecraft:{type_name}")
                };
                map.insert("type".to_string(), Value::String(full_type));
                map.insert("player".to_string(), nbt_compound_to_json(&profile.0));
                map.insert("hat".to_string(), Value::Bool(*hat));
            } else {
                let name = profile.0.get_string("name").unwrap_or("player_sprite");
                map.insert("text".to_string(), string(name));
            }
        }
    }
}

fn insert_style(map: &mut Map<String, Value>, style: &Style, version: JavaMinecraftVersion) {
    if let Some(ref color) = style.color {
        map.insert(
            "color".to_string(),
            Value::String(color_name(*color, version)),
        );
    }
    insert_flags(map, style);
    if version >= JavaMinecraftVersion::V_1_21_4
        && let Some(ref shadow) = style.shadow_color
    {
        map.insert(
            "shadow_color".to_string(),
            serde_json::json!(shadow.to_argb_int()),
        );
    }
    if let Some(ref click) = style.click_event {
        let key = if version >= JavaMinecraftVersion::V_1_21_5 {
            "click_event"
        } else {
            "clickEvent"
        };
        map.insert(key.to_string(), Value::Object(click_json(click, version)));
    }
    if let Some(ref hover) = style.hover_event {
        let key = if version >= JavaMinecraftVersion::V_1_21_5 {
            "hover_event"
        } else {
            "hoverEvent"
        };
        map.insert(key.to_string(), Value::Object(hover_json(hover, version)));
    }
}

fn color_name(color: Color, version: JavaMinecraftVersion) -> String {
    match color {
        Color::Reset => "reset".to_string(),
        Color::Named(c) => c.name().to_string(),
        Color::Rgb(rgb) => {
            if version >= JavaMinecraftVersion::V_1_16 {
                format!("#{:02X}{:02X}{:02X}", rgb.red, rgb.green, rgb.blue)
            } else {
                rgb.to_nearest_named().name().to_string()
            }
        }
    }
}

fn insert_flags(map: &mut Map<String, Value>, style: &Style) {
    let flags = [
        ("bold", style.bold),
        ("italic", style.italic),
        ("underlined", style.underlined),
        ("strikethrough", style.strikethrough),
        ("obfuscated", style.obfuscated),
    ];
    for (name, flag) in flags {
        if let Some(flag) = flag {
            map.insert(name.to_string(), Value::Bool(flag));
        }
    }
    if let Some(ref insertion) = style.insertion {
        map.insert("insertion".to_string(), Value::String(insertion.clone()));
    }
    if let Some(ref font) = style.font {
        map.insert("font".to_string(), Value::String(font.clone()));
    }
}

/// The action name, and the name of the field holding its argument from 1.21.5 on.
fn click_json(click: &ClickEvent, version: JavaMinecraftVersion) -> Map<String, Value> {
    let from_1_21_5 = version >= JavaMinecraftVersion::V_1_21_5;
    let (action, modern_field, argument) = match click {
        ClickEvent::OpenUrl { url } => ("open_url", Some("url"), string(url)),
        ClickEvent::OpenFile { path } => ("open_file", Some("path"), string(path)),
        ClickEvent::RunCommand { command } => ("run_command", Some("command"), string(command)),
        ClickEvent::SuggestCommand { command } => {
            ("suggest_command", Some("command"), string(command))
        }
        ClickEvent::ChangePage { page } => {
            let argument = if version >= JavaMinecraftVersion::V_1_21_6 {
                serde_json::json!(*page as i32)
            } else {
                string(page)
            };
            ("change_page", Some("page"), argument)
        }
        ClickEvent::CopyToClipboard { value } => ("copy_to_clipboard", None, string(value)),
    };
    let mut map = Map::new();
    map.insert("action".to_string(), string(action));
    let field = match modern_field {
        Some(field) if from_1_21_5 => field,
        _ => "value",
    };
    map.insert(field.to_string(), argument);
    map
}

fn hover_json(hover: &HoverEvent, version: JavaMinecraftVersion) -> Map<String, Value> {
    if version >= JavaMinecraftVersion::V_1_21_5 {
        hover_json_flat(hover, version)
    } else if version >= JavaMinecraftVersion::V_1_16 {
        hover_json_with_contents(hover, version)
    } else {
        hover_json_legacy(hover, version)
    }
}

/// 1.21.5 and later: the fields sit next to the action.
fn hover_json_flat(hover: &HoverEvent, version: JavaMinecraftVersion) -> Map<String, Value> {
    let mut map = Map::new();
    match hover {
        HoverEvent::ShowText { value } => {
            map.insert("action".to_string(), string("show_text"));
            map.insert("value".to_string(), one_or_array(value, version));
        }
        HoverEvent::ShowItem {
            id,
            count,
            components,
        } => {
            map.insert("action".to_string(), string("show_item"));
            map.insert("id".to_string(), string(id));
            if let Some(cnt) = count {
                map.insert("count".to_string(), serde_json::json!(*cnt));
            }
            if let Some(components) = components {
                map.insert("components".to_string(), components.0.clone());
            }
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            map.insert("action".to_string(), string("show_entity"));
            map.insert("id".to_string(), string(id));
            map.insert("uuid".to_string(), string(uuid));
            if let Some(n) = name {
                map.insert("name".to_string(), one_or_array(n, version));
            }
        }
    }
    map
}

/// 1.16 up to 1.21.4: the fields sit in a `contents` value.
fn hover_json_with_contents(
    hover: &HoverEvent,
    version: JavaMinecraftVersion,
) -> Map<String, Value> {
    let mut map = Map::new();
    match hover {
        HoverEvent::ShowText { value } => {
            map.insert("action".to_string(), string("show_text"));
            map.insert("contents".to_string(), one_or_array(value, version));
        }
        HoverEvent::ShowItem {
            id,
            count,
            components,
        } => {
            map.insert("action".to_string(), string("show_item"));
            let mut contents = Map::new();
            contents.insert("id".to_string(), string(id));
            if let Some(cnt) = count {
                contents.insert("count".to_string(), serde_json::json!(*cnt));
            }
            if let Some(components) = components {
                contents.insert("components".to_string(), components.0.clone());
            }
            map.insert("contents".to_string(), Value::Object(contents));
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            map.insert("action".to_string(), string("show_entity"));
            let mut contents = Map::new();
            contents.insert("type".to_string(), string(id));
            contents.insert("id".to_string(), string(uuid));
            if let Some(n) = name {
                contents.insert("name".to_string(), one_or_array(n, version));
            }
            map.insert("contents".to_string(), Value::Object(contents));
        }
    }
    map
}

/// Before 1.16: items and entities are one string in SNBT.
fn hover_json_legacy(hover: &HoverEvent, version: JavaMinecraftVersion) -> Map<String, Value> {
    let mut map = Map::new();
    match hover {
        HoverEvent::ShowText { value } => {
            map.insert("action".to_string(), string("show_text"));
            map.insert("value".to_string(), one_or_array(value, version));
        }
        HoverEvent::ShowItem { id, count, .. } => {
            map.insert("action".to_string(), string("show_item"));
            let count_val = count.unwrap_or(1);
            map.insert(
                "value".to_string(),
                Value::String(format!("{{id:\"{id}\",Count:{count_val}b}}")),
            );
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            map.insert("action".to_string(), string("show_entity"));
            let name_str = name.as_ref().map_or_else(String::new, |n| {
                n.iter()
                    .map(|e| e.clone().get_text(Locale::EnUs))
                    .collect::<String>()
            });
            map.insert(
                "value".to_string(),
                Value::String(format!(
                    "{{id:\"{uuid}\",type:\"{id}\",name:\"{name_str}\"}}"
                )),
            );
        }
    }
    map
}
