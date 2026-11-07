//! Characterisation of how text components become JSON and NBT, for every version that changes the
//! output. The golden file was written by the code before `to_json_value_for_version` and
//! `to_nbt_compound_for_version` were split into smaller functions; restructuring must not change it.
//! Rewrite it on purpose with `KVASIR_BLESS=1 cargo test -p pumpkin-util --test text_golden`.

use std::borrow::Cow;

use pumpkin_nbt::{Nbt, compound::NbtCompound, tag::NbtTag};
use pumpkin_util::text::{
    TextComponent, TextComponentBase, TextContent,
    click::ClickEvent,
    color::{ARGBColor, Color, NamedColor, RGBColor},
    hover::{HoverEvent, ItemComponents},
    style::Style,
};
use pumpkin_util::translation::Locale;
use pumpkin_util::version::JavaMinecraftVersion as V;

const VERSIONS: [V; 10] = [
    V::V_1_8,
    V::V_1_15_2,
    V::V_1_16,
    V::V_1_20_2,
    V::V_1_20_3,
    V::V_1_21_4,
    V::V_1_21_5,
    V::V_1_21_6,
    V::V_26_1,
    V::V_26_3,
];

fn text(value: &'static str) -> TextComponentBase {
    TextComponentBase {
        content: Box::new(TextContent::Text {
            text: Cow::Borrowed(value),
        }),
        style: Box::default(),
        extra: vec![],
    }
}

fn styled(value: &'static str, style: Style) -> TextComponentBase {
    TextComponentBase {
        style: Box::new(style),
        ..text(value)
    }
}

fn profile(named: bool) -> pumpkin_util::text::ProfileNbt {
    let mut compound = NbtCompound::new();
    if named {
        compound.put_string("name", "Steve".to_owned());
    }
    compound.put_string("id", "00000000".to_owned());
    pumpkin_util::text::ProfileNbt(compound)
}

fn text_contents() -> Vec<(&'static str, TextContent)> {
    vec![
        (
            "text",
            TextContent::Text {
                text: Cow::Borrowed("plain \"quoted\" ü"),
            },
        ),
        (
            "empty_text",
            TextContent::Text {
                text: Cow::Borrowed(""),
            },
        ),
    ]
}

fn translation_contents() -> Vec<(&'static str, TextContent)> {
    let with = || vec![text("first"), styled("second", bold())];
    vec![
        (
            "translate",
            TextContent::Translate {
                translate: Cow::Borrowed("chat.type.text"),
                bedrock_translate: None,
                with: with(),
            },
        ),
        (
            "translate_no_args",
            TextContent::Translate {
                translate: Cow::Borrowed("menu.quit"),
                bedrock_translate: Some(Cow::Borrowed("x")),
                with: vec![],
            },
        ),
        (
            "selector",
            TextContent::EntityNames {
                selector: Cow::Borrowed("@a"),
                separator: None,
            },
        ),
        (
            "selector_sep",
            TextContent::EntityNames {
                selector: Cow::Borrowed("@e[type=cow]"),
                separator: Some(Cow::Borrowed(", ")),
            },
        ),
        (
            "keybind",
            TextContent::Keybind {
                keybind: Cow::Borrowed("key.jump"),
            },
        ),
    ]
}

fn custom_contents() -> Vec<(&'static str, TextContent)> {
    let with = || vec![text("first"), styled("second", bold())];
    vec![
        (
            "custom",
            TextContent::Custom {
                key: Cow::Borrowed("custom.key"),
                locale: Locale::EnUs,
                with: with(),
            },
        ),
        (
            "custom_no_args",
            TextContent::Custom {
                key: Cow::Borrowed("custom.key"),
                locale: Locale::EnUs,
                with: vec![],
            },
        ),
    ]
}

fn sprite_contents() -> Vec<(&'static str, TextContent)> {
    vec![
        (
            "sprite",
            TextContent::PlayerSprite {
                type_name: Cow::Borrowed("player"),
                profile: profile(true),
                hat: true,
            },
        ),
        (
            "sprite_qualified",
            TextContent::PlayerSprite {
                type_name: Cow::Borrowed("minecraft:player"),
                profile: profile(true),
                hat: false,
            },
        ),
        (
            "sprite_unnamed",
            TextContent::PlayerSprite {
                type_name: Cow::Borrowed("player"),
                profile: profile(false),
                hat: true,
            },
        ),
    ]
}

fn contents() -> Vec<(&'static str, TextContent)> {
    let mut all = text_contents();
    all.extend(translation_contents());
    all.extend(custom_contents());
    all.extend(sprite_contents());
    all
}

fn bold() -> Style {
    Style {
        bold: Some(true),
        ..Style::default()
    }
}

fn with_hover(hover: HoverEvent) -> Style {
    Style {
        hover_event: Some(hover),
        ..Style::default()
    }
}

fn with_click(click: ClickEvent) -> Style {
    Style {
        click_event: Some(click),
        ..Style::default()
    }
}

fn color_styles() -> Vec<(&'static str, Style)> {
    vec![
        ("default", Style::default()),
        (
            "color_reset",
            Style {
                color: Some(Color::Reset),
                ..Style::default()
            },
        ),
        (
            "color_named",
            Style {
                color: Some(Color::Named(NamedColor::Red)),
                ..Style::default()
            },
        ),
        (
            "color_rgb",
            Style {
                color: Some(Color::Rgb(RGBColor {
                    red: 10,
                    green: 200,
                    blue: 255,
                })),
                ..Style::default()
            },
        ),
    ]
}

fn flag_styles() -> Vec<(&'static str, Style)> {
    vec![
        ("bold", bold()),
        (
            "italic_false",
            Style {
                italic: Some(false),
                ..Style::default()
            },
        ),
        (
            "underlined",
            Style {
                underlined: Some(true),
                ..Style::default()
            },
        ),
        (
            "strikethrough",
            Style {
                strikethrough: Some(true),
                ..Style::default()
            },
        ),
        (
            "obfuscated",
            Style {
                obfuscated: Some(false),
                ..Style::default()
            },
        ),
        (
            "insertion",
            Style {
                insertion: Some("ins".to_owned()),
                ..Style::default()
            },
        ),
        (
            "font",
            Style {
                font: Some("minecraft:uniform".to_owned()),
                ..Style::default()
            },
        ),
        (
            "shadow",
            Style {
                shadow_color: Some(ARGBColor {
                    alpha: 128,
                    red: 1,
                    green: 2,
                    blue: 3,
                }),
                ..Style::default()
            },
        ),
    ]
}

fn combined_styles() -> Vec<(&'static str, Style)> {
    vec![(
        "all_flags",
        Style {
            color: Some(Color::Named(NamedColor::Gold)),
            bold: Some(true),
            italic: Some(true),
            underlined: Some(false),
            strikethrough: Some(true),
            obfuscated: Some(true),
            insertion: Some("i".to_owned()),
            font: Some("f".to_owned()),
            shadow_color: Some(ARGBColor {
                alpha: 255,
                red: 9,
                green: 8,
                blue: 7,
            }),
            click_event: Some(ClickEvent::RunCommand {
                command: Cow::Borrowed("/c"),
            }),
            hover_event: Some(HoverEvent::ShowText {
                value: vec![text("h")],
            }),
        },
    )]
}

fn click_styles() -> Vec<(&'static str, Style)> {
    vec![
        (
            "click_url",
            with_click(ClickEvent::OpenUrl {
                url: Cow::Borrowed("https://example.org"),
            }),
        ),
        (
            "click_file",
            with_click(ClickEvent::OpenFile {
                path: Cow::Borrowed("/tmp/a"),
            }),
        ),
        (
            "click_run",
            with_click(ClickEvent::RunCommand {
                command: Cow::Borrowed("/say hi"),
            }),
        ),
        (
            "click_suggest",
            with_click(ClickEvent::SuggestCommand {
                command: Cow::Borrowed("/me"),
            }),
        ),
        ("click_page", with_click(ClickEvent::ChangePage { page: 3 })),
        (
            "click_copy",
            with_click(ClickEvent::CopyToClipboard {
                value: Cow::Borrowed("v"),
            }),
        ),
    ]
}

fn hover_styles() -> Result<Vec<(&'static str, Style)>, serde_json::Error> {
    let components: ItemComponents =
        serde_json::from_str(r#"{"minecraft:enchantments":{"minecraft:sharpness":5}}"#)?;
    Ok(vec![
        (
            "hover_text_1",
            with_hover(HoverEvent::ShowText {
                value: vec![text("tip")],
            }),
        ),
        (
            "hover_text_2",
            with_hover(HoverEvent::ShowText {
                value: vec![text("a"), styled("b", bold())],
            }),
        ),
        (
            "hover_item_bare",
            with_hover(HoverEvent::ShowItem {
                id: Cow::Borrowed("minecraft:stone"),
                count: None,
                components: None,
            }),
        ),
        (
            "hover_item_full",
            with_hover(HoverEvent::ShowItem {
                id: Cow::Borrowed("minecraft:diamond_sword"),
                count: Some(2),
                components: Some(components),
            }),
        ),
        (
            "hover_entity_bare",
            with_hover(HoverEvent::ShowEntity {
                id: Cow::Borrowed("minecraft:cow"),
                uuid: Cow::Borrowed("11111111-2222-3333-4444-555555555555"),
                name: None,
            }),
        ),
        (
            "hover_entity_1",
            with_hover(HoverEvent::ShowEntity {
                id: Cow::Borrowed("minecraft:cow"),
                uuid: Cow::Borrowed("11111111-2222-3333-4444-555555555555"),
                name: Some(vec![text("Bessie")]),
            }),
        ),
        (
            "hover_entity_2",
            with_hover(HoverEvent::ShowEntity {
                id: Cow::Borrowed("minecraft:cow"),
                uuid: Cow::Borrowed("11111111-2222-3333-4444-555555555555"),
                name: Some(vec![text("Bes"), text("sie")]),
            }),
        ),
    ])
}

fn styles() -> Result<Vec<(&'static str, Style)>, serde_json::Error> {
    let mut all = color_styles();
    all.extend(flag_styles());
    all.extend(click_styles());
    all.extend(hover_styles()?);
    all.extend(combined_styles());
    Ok(all)
}

/// Every case: a label and a component.
fn cases() -> Result<Vec<(String, TextComponentBase)>, serde_json::Error> {
    let mut all = Vec::new();
    let keep = ["default", "color_named", "all_flags"];
    for (content_name, content) in contents() {
        for (style_name, style) in styles()? {
            if !keep.contains(&style_name) {
                continue;
            }
            all.push((
                format!("{content_name}+{style_name}"),
                TextComponentBase {
                    content: Box::new(content.clone()),
                    style: Box::new(style),
                    extra: vec![],
                },
            ));
        }
    }
    for (style_name, style) in styles()? {
        for (content_name, content) in contents() {
            if content_name != "text" && content_name != "translate" {
                continue;
            }
            all.push((
                format!("{content_name}+{style_name}"),
                TextComponentBase {
                    content: Box::new(content),
                    style: Box::new(style.clone()),
                    extra: vec![],
                },
            ));
        }
    }
    for (name, extra) in [
        ("extra_plain", vec![text("a"), text("b")]),
        (
            "extra_styled",
            vec![
                styled("a", bold()),
                TextComponentBase {
                    extra: vec![text("deep")],
                    ..text("b")
                },
            ],
        ),
    ] {
        all.push((
            format!("text+default+{name}"),
            TextComponentBase {
                extra: extra.clone(),
                ..text("root")
            },
        ));
        all.push((
            format!("text+bold+{name}"),
            TextComponentBase {
                style: Box::new(bold()),
                extra,
                ..text("root")
            },
        ));
    }
    Ok(all)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn nbt_bytes(component: &TextComponentBase, version: V) -> Vec<u8> {
    let mut root = NbtCompound::new();
    root.put(
        "compound",
        NbtTag::Compound(component.to_nbt_compound_for_version(&version)),
    );
    root.put("tag", component.to_nbt_tag_for_version(&version));
    Nbt::new(String::new(), root).write_unnamed().to_vec()
}

fn lines() -> Result<Vec<String>, serde_json::Error> {
    let mut out = Vec::new();
    for (label, component) in cases()? {
        for version in VERSIONS {
            let json = component.to_json_value_for_version(&version).to_string();
            let wrapped = TextComponent(component.clone());
            let wrapped_json = wrapped.to_json_value_for_version(&version).to_string();
            let mut wrapped_root = NbtCompound::new();
            wrapped_root.put(
                "compound",
                NbtTag::Compound(wrapped.to_nbt_compound_for_version(&version)),
            );
            let wrapped_nbt = Nbt::new(String::new(), wrapped_root).write_unnamed();
            out.push(format!(
                "{label}|{version}|{:016x}|{:016x}|{:016x}|{:016x}",
                fnv(json.as_bytes()),
                fnv(&nbt_bytes(&component, version)),
                fnv(wrapped_json.as_bytes()),
                fnv(&wrapped_nbt)
            ));
        }
    }
    Ok(out)
}

#[test]
fn text_components_serialise_as_before_for_every_version() -> Result<(), Box<dyn std::error::Error>>
{
    let now = lines()?.join("\n") + "\n";
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/text_components.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    let golden = std::fs::read_to_string(&path)?;
    let differing: Vec<(&str, &str)> = golden
        .lines()
        .zip(now.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        golden.lines().count(),
        now.lines().count(),
        "number of cases"
    );
    assert!(
        differing.is_empty(),
        "{} cases differ, first: {:?}",
        differing.len(),
        differing.first()
    );
    Ok(())
}
