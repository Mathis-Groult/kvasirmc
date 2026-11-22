//! Text rendering of the dispenser golden's scenes.

use std::fmt::Write as _;
use std::sync::{Arc, PoisonError};

use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::java::client::play::{CSoundEffect, CWorldEvent};
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;

use crate::block::entities::dispenser::DispenserBlockEntity;
use crate::entity::EntityBase;
use crate::entity::item::ItemEntity;
use crate::entity::passive::sheep::SheepEntity;
use crate::net::java::PacketTap;
use crate::world::World;

pub(super) fn stack_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        return "empty".to_string();
    }
    let mut text = format!("{} x{}", stack.item.registry_key, stack.item_count);
    for (component, value) in &stack.patch {
        let _ = write!(
            text,
            " {component:?}{}",
            if value.is_some() { "" } else { "-" }
        );
    }
    let damage = stack.get_damage();
    if damage != 0 {
        let _ = write!(text, " damage {damage}");
    }
    text
}

pub(super) fn var_int(bytes: &[u8], mut at: usize) -> Option<(i32, usize)> {
    let mut value = 0i32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(at)?;
        at += 1;
        value |= i32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
    }
    None
}

pub(super) fn hex(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// What the listener heard: world events whole, sounds without their random pitch and seed,
/// other packets ignored (spawn packets arrive on a tick).
pub(super) fn heard(tap: &mut PacketTap) -> String {
    let event_id = CWorldEvent::to_id(CURRENT_MC_VERSION);
    let sound_id = CSoundEffect::to_id(CURRENT_MC_VERSION);
    let mut text = String::new();
    for packet in tap.drain() {
        let first = var_int(&packet, 0);
        let second = first.and_then(|(_, next)| var_int(&packet, next));
        let is = |id: i32| {
            first.is_some_and(|(value, _)| value == id)
                || second.is_some_and(|(value, _)| value == id)
        };
        if is(event_id) {
            let _ = write!(text, " event {}", hex(&packet));
        } else if is(sound_id) && packet.len() >= 12 {
            let _ = write!(text, " sound {}", hex(&packet[..packet.len() - 12]));
        }
    }
    text
}

const FLYERS: [&str; 10] = [
    "arrow",
    "spectral_arrow",
    "snowball",
    "egg",
    "splash_potion",
    "lingering_potion",
    "small_fireball",
    "wind_charge",
    "firework_rocket",
    "experience_bottle",
];

pub(super) fn flight(velocity: Vector3<f64>) -> &'static str {
    let (x, y, z) = (velocity.x.abs(), velocity.y.abs(), velocity.z.abs());
    if x.max(y).max(z) < 1e-9 {
        "still"
    } else if x >= y && x >= z {
        if velocity.x > 0.0 { "+x" } else { "-x" }
    } else if y >= z {
        if velocity.y > 0.0 { "+y" } else { "-y" }
    } else if velocity.z > 0.0 {
        "+z"
    } else {
        "-z"
    }
}

pub(super) fn entity_text(entity: &Arc<dyn EntityBase>, loose: bool) -> String {
    let base = entity.get_entity();
    let at = base.pos.load();
    let name = base.entity_type.resource_name;
    let mut text = format!("{name} ({:.3}, {:.3}, {:.3})", at.x, at.y, at.z);
    if let Some(dropped) = entity.cast_any().downcast_ref::<ItemEntity>() {
        let stack = dropped
            .get_item_stack()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if loose {
            let key = stack.item.registry_key;
            let _ = write!(
                text,
                " {}",
                if key.ends_with("_wool") { "wool" } else { key }
            );
        } else {
            let _ = write!(text, " {}", stack_text(&stack));
        }
    } else if FLYERS.contains(&name) {
        let _ = write!(text, " flies {}", flight(base.velocity.load()));
    }
    text
}

pub(super) fn equipment_text(entity: &dyn EntityBase) -> String {
    let mut text = entity.get_entity().entity_type.resource_name.to_string();
    if let Some(sheep) = entity.cast_any().downcast_ref::<SheepEntity>() {
        let _ = write!(text, " sheared {}", sheep.is_sheared());
    }
    if let Some(living) = entity.get_living_entity() {
        let equipment = living
            .entity_equipment
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (label, slot) in [
            ("feet", EquipmentSlot::FEET),
            ("legs", EquipmentSlot::LEGS),
            ("chest", EquipmentSlot::CHEST),
            ("head", EquipmentSlot::HEAD),
            ("body", EquipmentSlot::BODY),
            ("saddle", EquipmentSlot::SADDLE),
        ] {
            let worn = equipment.get(&slot);
            if !worn.is_empty() {
                let _ = write!(text, " {label}={}", stack_text(&worn));
            }
        }
    }
    text
}

/// Every block of the region around the dispenser.
pub(super) fn region(world: &World, pos: BlockPos) -> Vec<(BlockPos, BlockStateId)> {
    let mut blocks = Vec::new();
    for dx in -3..=3 {
        for dy in -3..=4 {
            for dz in -3..=3 {
                let at = BlockPos::new(pos.0.x + dx, pos.0.y + dy, pos.0.z + dz);
                blocks.push((at, world.get_block_state_id(&at)));
            }
        }
    }
    blocks
}

pub(super) fn changes_text(world: &World, pos: BlockPos, before: &[(BlockPos, BlockStateId)]) -> String {
    let mut text = String::new();
    for (at, was) in before {
        let now = world.get_block_state_id(at);
        if now != *was {
            let _ = write!(
                text,
                " {},{},{}: {}#{} -> {}#{};",
                at.0.x - pos.0.x,
                at.0.y - pos.0.y,
                at.0.z - pos.0.z,
                Block::from_state_id(*was).name,
                was.as_u16(),
                world.get_block(at).name,
                now.as_u16(),
            );
        }
    }
    text
}

pub(super) fn slots_text(dispenser: &DispenserBlockEntity) -> String {
    let items = dispenser
        .items
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let mut text = String::new();
    for (index, stack) in items.iter().enumerate() {
        if !stack.is_empty() {
            let _ = write!(text, " {index}:{}", stack_text(stack));
        }
    }
    text
}
