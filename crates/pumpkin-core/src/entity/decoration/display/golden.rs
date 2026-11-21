//! A record of the display entity's NBT read, NBT write and tracked data
//! (`tests/golden/display_entity.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made before
//! `read_display_nbt`, `write_display_nbt` and `init_display_data_tracker` were split.
//!
//! Per case: a fresh display entity reads one NBT compound; the line holds what it writes back
//! (as SNBT text) and the tracked values it sends, one `index=hex` per entry in index order.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::entity::EntityType;
use pumpkin_data::tracked_data::display;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::version::JavaMinecraftVersion;

use super::DisplayEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::world::World;

fn floats(values: &[f32]) -> NbtTag {
    NbtTag::List(values.iter().map(|v| NbtTag::Float(*v)).collect())
}

fn compound(entries: Vec<(&str, NbtTag)>) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    for (key, value) in entries {
        nbt.put(key, value);
    }
    nbt
}

fn brightness(block: Option<i32>, sky: Option<i32>) -> NbtTag {
    let mut entries = Vec::new();
    if let Some(block) = block {
        entries.push(("block", NbtTag::Int(block)));
    }
    if let Some(sky) = sky {
        entries.push(("sky", NbtTag::Int(sky)));
    }
    NbtTag::Compound(compound(entries))
}

fn transformation(entries: Vec<(&str, NbtTag)>) -> NbtTag {
    NbtTag::Compound(compound(entries))
}

fn full() -> NbtCompound {
    compound(vec![
        ("interpolation_duration", NbtTag::Int(7)),
        ("start_interpolation", NbtTag::Int(-3)),
        ("teleport_duration", NbtTag::Int(5)),
        ("view_range", NbtTag::Float(2.5)),
        ("shadow_radius", NbtTag::Float(0.75)),
        ("shadow_strength", NbtTag::Float(0.5)),
        ("width", NbtTag::Float(3.0)),
        ("height", NbtTag::Float(4.0)),
        ("glow_color_override", NbtTag::Int(0x00ff_8800)),
        ("billboard", NbtTag::String("center".into())),
        ("brightness", brightness(Some(12), Some(5))),
        (
            "transformation",
            transformation(vec![
                ("translation", floats(&[1.0, -2.0, 0.5])),
                ("scale", floats(&[2.0, 3.0, 4.0])),
                ("left_rotation", floats(&[0.0, 0.7, 0.0, 0.7])),
                ("right_rotation", floats(&[0.5, 0.5, 0.5, 0.5])),
            ]),
        ),
    ])
}

fn cases() -> Vec<(&'static str, NbtCompound)> {
    let mut list = vec![("empty", NbtCompound::new()), ("full", full())];
    for billboard in ["vertical", "horizontal", "center", "fixed", "sideways"] {
        let name: &'static str = match billboard {
            "vertical" => "billboard vertical",
            "horizontal" => "billboard horizontal",
            "center" => "billboard center",
            "fixed" => "billboard fixed",
            _ => "billboard unknown",
        };
        list.push((
            name,
            compound(vec![("billboard", NbtTag::String(billboard.into()))]),
        ));
    }
    list.extend([
        (
            "brightness 15/15",
            compound(vec![("brightness", brightness(Some(15), Some(15)))]),
        ),
        (
            "brightness block only",
            compound(vec![("brightness", brightness(Some(7), None))]),
        ),
        (
            "brightness sky only",
            compound(vec![("brightness", brightness(None, Some(9)))]),
        ),
        (
            "brightness block 20",
            compound(vec![("brightness", brightness(Some(20), Some(1)))]),
        ),
        (
            "teleport_duration clamped",
            compound(vec![("teleport_duration", NbtTag::Int(100))]),
        ),
        (
            "glow only",
            compound(vec![("glow_color_override", NbtTag::Int(0x0012_3456))]),
        ),
        (
            "wrong types",
            compound(vec![
                ("view_range", NbtTag::Int(9)),
                ("interpolation_duration", NbtTag::Float(2.0)),
                ("billboard", NbtTag::Int(1)),
                ("brightness", NbtTag::Int(255)),
            ]),
        ),
    ]);
    list.extend(transformation_cases());
    // Appended after the mutation run: a block light above 15 with no sky light.
    list.push((
        "brightness block 20 sky 0",
        compound(vec![("brightness", brightness(Some(20), Some(0)))]),
    ));
    list
}

/// Transformation lists that are too short, too long or hold other tag types.
fn transformation_cases() -> Vec<(&'static str, NbtCompound)> {
    vec![
        (
            "short lists",
            compound(vec![(
                "transformation",
                transformation(vec![
                    ("translation", floats(&[1.0, 2.0])),
                    ("scale", floats(&[5.0, 6.0])),
                    ("left_rotation", floats(&[0.1, 0.2, 0.3])),
                    ("right_rotation", floats(&[0.4, 0.5, 0.6])),
                ]),
            )]),
        ),
        (
            "non-float entries",
            compound(vec![(
                "transformation",
                transformation(vec![
                    (
                        "translation",
                        NbtTag::List(vec![NbtTag::Int(1), NbtTag::Float(2.0), NbtTag::Int(3)]),
                    ),
                    (
                        "scale",
                        NbtTag::List(vec![
                            NbtTag::String("x".into()),
                            NbtTag::Float(6.0),
                            NbtTag::Byte(1),
                        ]),
                    ),
                    (
                        "left_rotation",
                        NbtTag::List(vec![
                            NbtTag::Int(0),
                            NbtTag::Int(0),
                            NbtTag::Int(0),
                            NbtTag::Int(0),
                        ]),
                    ),
                    (
                        "right_rotation",
                        NbtTag::List(vec![
                            NbtTag::Float(0.1),
                            NbtTag::Float(0.2),
                            NbtTag::Float(0.3),
                            NbtTag::Short(1),
                        ]),
                    ),
                ]),
            )]),
        ),
        (
            "long lists",
            compound(vec![(
                "transformation",
                transformation(vec![
                    ("translation", floats(&[1.0, 2.0, 3.0, 4.0])),
                    ("right_rotation", floats(&[0.1, 0.2, 0.3, 0.4, 0.5])),
                ]),
            )]),
        ),
    ]
}

/// Value length of each display entry after its index and type bytes.
fn value_length(index: u8, bytes: &[u8]) -> usize {
    let float_ids = [
        display::VIEW_RANGE.id.0,
        display::SHADOW_RADIUS.id.0,
        display::SHADOW_STRENGTH.id.0,
        display::WIDTH.id.0,
        display::HEIGHT.id.0,
    ];
    if float_ids.contains(&index) {
        4
    } else if index == display::TRANSLATION.id.0 || index == display::SCALE.id.0 {
        12
    } else if index == display::LEFT_ROTATION.id.0 || index == display::RIGHT_ROTATION.id.0 {
        16
    } else if index == display::BILLBOARD.id.0 {
        1
    } else {
        // VarInt: up to and including the first byte without the continuation bit.
        bytes
            .iter()
            .position(|byte| byte & 0x80 == 0)
            .map_or(bytes.len(), |end| end + 1)
    }
}

/// The tracked entries the entity would send, sorted by index.
fn tracked_entries(entity: &Entity) -> String {
    let Some(bytes) = entity
        .synched_data
        .pack_dirty_for_version(&JavaMinecraftVersion::V_26_3)
    else {
        return "none".to_string();
    };
    let mut entries = Vec::new();
    let mut at = 0;
    while at < bytes.len() && bytes[at] != 0xff {
        let index = bytes[at];
        let start = at + 2;
        let end = start + value_length(index, &bytes[start..]);
        let hex: String = bytes[at + 1..end]
            .iter()
            .fold(String::new(), |mut text, byte| {
                let _ = write!(text, "{byte:02x}");
                text
            });
        entries.push((index, hex));
        at = end;
    }
    if at + 1 != bytes.len() {
        return format!("unparsed tail at {at} of {}", bytes.len());
    }
    entries.sort();
    entries
        .iter()
        .map(|(index, hex)| format!("{index}={hex}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A tag as text with compound keys sorted, so the line does not depend on hash order.
fn tag_text(tag: &NbtTag) -> String {
    match tag {
        NbtTag::Compound(compound) => {
            let mut keys: Vec<&Box<str>> = compound.child_tags.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{key}: {}", tag_text(&compound.child_tags[key])))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
        NbtTag::List(items) => {
            let items: Vec<String> = items.iter().map(tag_text).collect();
            format!("[{}]", items.join(", "))
        }
        other => format!("{other:?}"),
    }
}

fn record(world: &Arc<World>, nbt: &NbtCompound) -> String {
    let entity = Entity::new(
        Arc::clone(world),
        Vector3::new(0.5, 64.0, 0.5),
        &EntityType::BLOCK_DISPLAY,
    );
    let display = DisplayEntity::new(entity);
    display.read_display_nbt(nbt);
    let mut written = NbtCompound::new();
    display.write_display_nbt(&mut written);
    display.init_display_data_tracker();
    format!(
        "wrote {} | tracked {}",
        tag_text(&NbtTag::Compound(written)),
        tracked_entries(&display.entity)
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn display_entity_nbt_and_tracker_are_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();

    let mut now = String::new();
    for (name, nbt) in cases() {
        writeln!(now, "{name}: {}", record(&world, &nbt))?;
    }

    // Appended after the mutation run: the teleport duration cannot come from NBT (see #69),
    // so it is set directly before writing and tracking.
    let entity = Entity::new(
        Arc::clone(&world),
        Vector3::new(0.5, 64.0, 0.5),
        &EntityType::BLOCK_DISPLAY,
    );
    let display = DisplayEntity::new(entity);
    display.set_teleport_duration(9);
    let mut written = NbtCompound::new();
    display.write_display_nbt(&mut written);
    display.init_display_data_tracker();
    writeln!(
        now,
        "teleport set to 9: wrote {} | tracked {}",
        tag_text(&NbtTag::Compound(written)),
        tracked_entries(&display.entity)
    )?;

    script_golden::bless_or_compare("display_entity.txt", &now)?;
    server.stop();
    Ok(())
}

#[test]
fn brightness_nbt_is_packed_like_vanilla_and_rejects_invalid_compounds() {
    let nbt = |block: Option<i32>, sky: Option<i32>| match brightness(block, sky) {
        NbtTag::Compound(c) => c,
        _ => NbtCompound::new(),
    };
    assert_eq!(
        super::light::brightness_from_nbt(&nbt(Some(15), Some(15))),
        Some(15 << 4 | 15 << 20)
    );
    assert_eq!(
        super::light::brightness_from_nbt(&nbt(Some(7), Some(0))),
        Some(7 << 4)
    );
    assert_eq!(super::light::brightness_from_nbt(&nbt(Some(7), None)), None);
    assert_eq!(super::light::brightness_from_nbt(&nbt(None, Some(9))), None);
    assert_eq!(
        super::light::brightness_from_nbt(&nbt(Some(20), Some(1))),
        None
    );
    assert_eq!(
        super::light::brightness_from_nbt(&nbt(Some(-1), Some(1))),
        None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reading_an_empty_compound_resets_every_field_to_its_default() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let new_display = || {
        DisplayEntity::new(Entity::new(
            Arc::clone(&world),
            Vector3::new(0.5, 64.0, 0.5),
            &EntityType::BLOCK_DISPLAY,
        ))
    };
    let written = |display: &DisplayEntity| {
        let mut out = NbtCompound::new();
        display.write_display_nbt(&mut out);
        tag_text(&NbtTag::Compound(out))
    };
    let used = new_display();
    used.read_display_nbt(&full());
    assert_ne!(written(&used), written(&new_display()));
    used.read_display_nbt(&NbtCompound::new());
    assert_eq!(written(&used), written(&new_display()));
    server.stop();
    Ok(())
}

/// What `init_data_tracker` of a display entity sends, without its terminating 0xff.
fn tracked_bytes(entity: &Entity) -> Vec<u8> {
    let mut bytes = entity
        .synched_data
        .pack_dirty_for_version(&JavaMinecraftVersion::V_26_3)
        .map_or_else(Vec::new, Vec::from);
    bytes.pop();
    bytes
}

/// The Block, Item and Text wrappers must call the shared display NBT read, write and tracker
/// (`kvasir-test --extra-mutants` deleted each call unseen, card #70), and add their own data.
#[tokio::test(flavor = "multi_thread")]
async fn each_display_wrapper_delegates_to_the_shared_display_code() -> TestResult {
    use crate::entity::EntityBase;

    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let entity = |kind| Entity::new(Arc::clone(&world), Vector3::new(0.5, 64.0, 0.5), kind);

    let plain = DisplayEntity::new(entity(&EntityType::BLOCK_DISPLAY));
    plain.init_display_data_tracker();
    let plain_bytes = tracked_bytes(&plain.entity);
    assert!(!plain_bytes.is_empty());

    let block = super::BlockDisplayEntity::new(entity(&EntityType::BLOCK_DISPLAY));
    let item = super::ItemDisplayEntity::new(entity(&EntityType::ITEM_DISPLAY));
    let text = super::TextDisplayEntity::new(entity(&EntityType::TEXT_DISPLAY));
    let wrappers: [(&str, &dyn EntityBase, &DisplayEntity); 3] = [
        ("block", &*block, &block.display),
        ("item", &*item, &item.display),
        ("text", &*text, &text.display),
    ];
    for (name, wrapper, display) in wrappers {
        wrapper.read_custom_nbt(&full());
        let mut written = NbtCompound::new();
        wrapper.write_custom_nbt(&mut written);
        assert_eq!(
            written.get_int("interpolation_duration"),
            Some(7),
            "{name} write"
        );
        assert_eq!(
            written.get_int("teleport_duration"),
            Some(5),
            "{name} write"
        );
        assert_eq!(
            display.interpolation_duration.load(Ordering::Relaxed),
            7,
            "{name} read"
        );

        wrapper.init_data_tracker();
        let bytes = tracked_bytes(&display.entity);
        assert!(bytes.len() > plain_bytes.len(), "{name} adds its own data");
        let view_range = 2.5f32.to_be_bytes();
        assert!(
            bytes.windows(4).any(|w| w == view_range),
            "{name} tracker holds the values read"
        );
    }
    server.stop();
    Ok(())
}
