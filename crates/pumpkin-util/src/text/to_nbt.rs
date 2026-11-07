//! A text component as the NBT compound the Java protocol sends, which changes shape with the client
//! version. It mirrors `to_json.rs`, with bytes for flags and the same key names.

use pumpkin_nbt::NbtCompound;
use pumpkin_nbt::tag::NbtTag;

use super::click::ClickEvent;
use super::color::Color;
use super::hover::HoverEvent;
use super::style::Style;
use super::{TextComponentBase, TextContent};
use crate::translation::Locale;
use crate::version::JavaMinecraftVersion;

impl TextComponentBase {
    /// Converts this component to an NBT compound tag for a specific Minecraft version.
    #[must_use]
    pub fn to_nbt_compound_for_version(&self, version: &JavaMinecraftVersion) -> NbtCompound {
        let mut compound = NbtCompound::new();
        put_content(&mut compound, &self.content, *version);
        put_style(&mut compound, &self.style, *version);
        if !self.extra.is_empty() {
            compound.put_list("extra", tags(&self.extra, *version));
        }
        compound
    }
}

fn tags(components: &[TextComponentBase], version: JavaMinecraftVersion) -> Vec<NbtTag> {
    components
        .iter()
        .map(|component| component.to_nbt_tag_for_version(&version))
        .collect()
}

/// One component as itself, several as a list.
fn put_one_or_list(
    compound: &mut NbtCompound,
    key: &str,
    components: &[TextComponentBase],
    version: JavaMinecraftVersion,
) {
    if components.len() == 1 {
        compound.put(key, components[0].to_nbt_tag_for_version(&version));
    } else {
        compound.put_list(key, tags(components, version));
    }
}

fn put_translation(
    compound: &mut NbtCompound,
    key: &str,
    with: &[TextComponentBase],
    version: JavaMinecraftVersion,
) {
    compound.put_string("translate", key.to_string());
    if !with.is_empty() {
        compound.put_list("with", tags(with, version));
    }
}

fn put_content(compound: &mut NbtCompound, content: &TextContent, version: JavaMinecraftVersion) {
    match content {
        TextContent::Text { text } => compound.put_string("text", text.to_string()),
        TextContent::Translate {
            translate, with, ..
        } => put_translation(compound, translate, with, version),
        TextContent::EntityNames {
            selector,
            separator,
        } => {
            compound.put_string("selector", selector.to_string());
            if let Some(sep) = separator {
                compound.put_string("separator", sep.to_string());
            }
        }
        TextContent::Keybind { keybind } => compound.put_string("keybind", keybind.to_string()),
        TextContent::Custom { key, with, .. } => put_translation(compound, key, with, version),
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
                compound.put_string("type", full_type);
                compound.put_compound("player", profile.0.clone());
                compound.put_byte("hat", i8::from(*hat));
            } else {
                let name = profile.0.get_string("name").unwrap_or("player_sprite");
                compound.put_string("text", name.to_string());
            }
        }
    }
}

fn put_style(compound: &mut NbtCompound, style: &Style, version: JavaMinecraftVersion) {
    if let Some(ref color) = style.color {
        compound.put_string("color", color_name(*color, version));
    }
    put_flags(compound, style);
    if version >= JavaMinecraftVersion::V_1_21_4
        && let Some(ref shadow) = style.shadow_color
    {
        compound.put_int("shadow_color", shadow.to_argb_int());
    }
    if let Some(ref click) = style.click_event {
        let key = if version >= JavaMinecraftVersion::V_1_21_5 {
            "click_event"
        } else {
            "clickEvent"
        };
        compound.put_compound(key, click_tag(click, version));
    }
    if let Some(ref hover) = style.hover_event {
        let key = if version >= JavaMinecraftVersion::V_1_21_5 {
            "hover_event"
        } else {
            "hoverEvent"
        };
        compound.put_compound(key, hover_tag(hover, version));
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

fn put_flags(compound: &mut NbtCompound, style: &Style) {
    let flags = [
        ("bold", style.bold),
        ("italic", style.italic),
        ("underlined", style.underlined),
        ("strikethrough", style.strikethrough),
        ("obfuscated", style.obfuscated),
    ];
    for (name, flag) in flags {
        if let Some(flag) = flag {
            compound.put_byte(name, i8::from(flag));
        }
    }
    if let Some(ref insertion) = style.insertion {
        compound.put_string("insertion", insertion.clone());
    }
    if let Some(ref font) = style.font {
        compound.put_string("font", font.clone());
    }
}

/// The action name, and the name of the field holding its argument from 1.21.5 on.
fn click_tag(click: &ClickEvent, version: JavaMinecraftVersion) -> NbtCompound {
    let from_1_21_5 = version >= JavaMinecraftVersion::V_1_21_5;
    let mut tag = NbtCompound::new();
    let (action, modern_field, argument) = match click {
        ClickEvent::OpenUrl { url } => ("open_url", Some("url"), url.to_string()),
        ClickEvent::OpenFile { path } => ("open_file", Some("path"), path.to_string()),
        ClickEvent::RunCommand { command } => ("run_command", Some("command"), command.to_string()),
        ClickEvent::SuggestCommand { command } => {
            ("suggest_command", Some("command"), command.to_string())
        }
        ClickEvent::ChangePage { page } => {
            tag.put_string("action", "change_page".to_string());
            if version >= JavaMinecraftVersion::V_1_21_6 {
                tag.put_int("page", *page as i32);
            } else if from_1_21_5 {
                tag.put_string("page", page.to_string());
            } else {
                tag.put_string("value", page.to_string());
            }
            return tag;
        }
        ClickEvent::CopyToClipboard { value } => ("copy_to_clipboard", None, value.to_string()),
    };
    tag.put_string("action", action.to_string());
    let field = match modern_field {
        Some(field) if from_1_21_5 => field,
        _ => "value",
    };
    tag.put_string(field, argument);
    tag
}

fn hover_tag(hover: &HoverEvent, version: JavaMinecraftVersion) -> NbtCompound {
    if version >= JavaMinecraftVersion::V_1_21_5 {
        hover_tag_flat(hover, version)
    } else if version >= JavaMinecraftVersion::V_1_16 {
        hover_tag_with_contents(hover, version)
    } else {
        hover_tag_legacy(hover, version)
    }
}

/// 1.21.5 and later: the fields sit next to the action.
fn hover_tag_flat(hover: &HoverEvent, version: JavaMinecraftVersion) -> NbtCompound {
    let mut tag = NbtCompound::new();
    match hover {
        HoverEvent::ShowText { value } => {
            tag.put_string("action", "show_text".to_string());
            put_one_or_list(&mut tag, "value", value, version);
        }
        HoverEvent::ShowItem {
            id,
            count,
            components,
        } => {
            tag.put_string("action", "show_item".to_string());
            tag.put_string("id", id.to_string());
            if let Some(cnt) = count {
                tag.put_int("count", *cnt);
            }
            if let Some(components) = components {
                tag.put("components", components.to_nbt());
            }
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            tag.put_string("action", "show_entity".to_string());
            tag.put_string("id", id.to_string());
            tag.put_string("uuid", uuid.to_string());
            if let Some(n) = name {
                put_one_or_list(&mut tag, "name", n, version);
            }
        }
    }
    tag
}

/// 1.16 up to 1.21.4: the fields sit in a `contents` value.
fn hover_tag_with_contents(hover: &HoverEvent, version: JavaMinecraftVersion) -> NbtCompound {
    let mut tag = NbtCompound::new();
    match hover {
        HoverEvent::ShowText { value } => {
            tag.put_string("action", "show_text".to_string());
            put_one_or_list(&mut tag, "contents", value, version);
        }
        HoverEvent::ShowItem {
            id,
            count,
            components,
        } => {
            tag.put_string("action", "show_item".to_string());
            let mut contents = NbtCompound::new();
            contents.put_string("id", id.to_string());
            if let Some(cnt) = count {
                contents.put_int("count", *cnt);
            }
            if let Some(components) = components {
                contents.put("components", components.to_nbt());
            }
            tag.put_compound("contents", contents);
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            tag.put_string("action", "show_entity".to_string());
            let mut contents = NbtCompound::new();
            contents.put_string("type", id.to_string());
            contents.put_string("id", uuid.to_string());
            if let Some(n) = name {
                put_one_or_list(&mut contents, "name", n, version);
            }
            tag.put_compound("contents", contents);
        }
    }
    tag
}

/// Before 1.16: items and entities are one string in SNBT.
fn hover_tag_legacy(hover: &HoverEvent, version: JavaMinecraftVersion) -> NbtCompound {
    let mut tag = NbtCompound::new();
    match hover {
        HoverEvent::ShowText { value } => {
            tag.put_string("action", "show_text".to_string());
            put_one_or_list(&mut tag, "value", value, version);
        }
        HoverEvent::ShowItem { id, count, .. } => {
            tag.put_string("action", "show_item".to_string());
            let count_val = count.unwrap_or(1);
            tag.put_string("value", format!("{{id:\"{id}\",Count:{count_val}b}}"));
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            tag.put_string("action", "show_entity".to_string());
            let name_str = name.as_ref().map_or_else(String::new, |n| {
                n.iter()
                    .map(|e| e.clone().get_text(Locale::EnUs))
                    .collect::<String>()
            });
            tag.put_string(
                "value",
                format!("{{id:\"{uuid}\",type:\"{id}\",name:\"{name_str}\"}}"),
            );
        }
    }
    tag
}
