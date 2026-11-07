//! Custom data and NBT reading and writing.

use super::{
    Entity, MAX_HORIZONTAL_POSITION, MAX_LOADED_MOTION, MAX_SCOREBOARD_TAGS, MAX_VERTICAL_POSITION,
    breath,
};
use pumpkin_data::entity::EntityType;
use pumpkin_data::tracked_data;
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use std::sync::atomic::Ordering::Relaxed;

impl Entity {
    pub fn set_custom_data(&self, namespace: &str, key: &str, value: NbtTag) {
        let mut custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut namespace_data = custom_data
            .child_tags
            .remove(namespace)
            .and_then(|tag| match tag {
                NbtTag::Compound(compound) => Some(compound),
                _ => None,
            })
            .unwrap_or_default();

        namespace_data.child_tags.insert(key.into(), value);
        custom_data
            .child_tags
            .insert(namespace.into(), NbtTag::Compound(namespace_data));
    }

    pub fn get_custom_data(&self, namespace: &str, key: &str) -> Option<NbtTag> {
        let custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        custom_data
            .get(namespace)?
            .extract_compound()?
            .get(key)
            .cloned()
    }

    pub fn remove_custom_data(&self, namespace: &str, key: &str) {
        let mut custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let Some(NbtTag::Compound(mut namespace_data)) = custom_data.child_tags.remove(namespace)
        else {
            return;
        };

        namespace_data.child_tags.remove(key);
        if !namespace_data.is_empty() {
            custom_data
                .child_tags
                .insert(namespace.into(), NbtTag::Compound(namespace_data));
        }
    }

    pub fn has_custom_data(&self, namespace: &str, key: &str) -> bool {
        self.get_custom_data(namespace, key).is_some()
    }

    pub fn write_nbt(&self, nbt: &mut NbtCompound) {
        let position = self.pos.load();
        nbt.put_string(
            "id",
            format!("minecraft:{}", self.entity_type.resource_name),
        );
        nbt.put_uuid("UUID", self.entity_uuid);
        let (x, z) = self
            .get_vehicle()
            .map_or((position.x, position.z), |vehicle| {
                let vehicle_pos = vehicle.get_entity().pos.load();
                (vehicle_pos.x, vehicle_pos.z)
            });
        nbt.put(
            "Pos",
            NbtTag::List(vec![x.into(), position.y.into(), z.into()]),
        );
        let velocity = self.velocity.load();
        nbt.put(
            "Motion",
            NbtTag::List(vec![
                velocity.x.into(),
                velocity.y.into(),
                velocity.z.into(),
            ]),
        );
        nbt.put(
            "Rotation",
            NbtTag::List(vec![self.yaw.load().into(), self.pitch.load().into()]),
        );
        if self.entity_type != &EntityType::PLAYER {
            nbt.put_short("Air", self.air_supply.load(Relaxed) as i16);
        }
        nbt.put_short("Fire", self.fire_ticks.load(Relaxed) as i16);
        nbt.put_bool("OnGround", self.on_ground.load(Relaxed));
        nbt.put_bool("Invulnerable", self.invulnerable.load(Relaxed));
        nbt.put_int("PortalCooldown", self.portal_cooldown.load(Relaxed) as i32);
        if let Some(custom_name) = &**self.custom_name.load() {
            let mut tag = custom_name
                .to_nbt_tag_for_version(&pumpkin_util::version::JavaMinecraftVersion::V_26_3);
            // A literal string starting with '{' would read back as legacy JSON, so keep it a compound.
            if let NbtTag::String(text) = &tag
                && text.starts_with('{')
            {
                let mut literal = NbtCompound::new();
                literal.put_string("text", text.to_string());
                tag = NbtTag::Compound(literal);
            }
            nbt.put("CustomName", tag);
        }
        if self.custom_name_visible.load(Relaxed) {
            nbt.put_bool("CustomNameVisible", true);
        }
        if self.is_silent() {
            nbt.put_bool("Silent", true);
        }
        if self.has_no_gravity() {
            nbt.put_bool("NoGravity", true);
        }
        if self.has_glowing_tag.load(Relaxed) {
            nbt.put_bool("Glowing", true);
        }
        let frozen_ticks = self.frozen_ticks.load(Relaxed);
        if frozen_ticks > 0 {
            nbt.put_int("TicksFrozen", frozen_ticks);
        }
        if self.has_visual_fire.load(Relaxed) {
            nbt.put_bool("HasVisualFire", true);
        }

        let tags = self
            .scoreboard_tags
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !tags.is_empty() {
            nbt.put(
                "Tags",
                NbtTag::List(
                    tags.iter()
                        .map(|tag| NbtTag::String(tag.as_str().into()))
                        .collect(),
                ),
            );
        }

        let custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !custom_data.is_empty() {
            nbt.put_compound("data", custom_data.clone());
        }
    }

    pub(super) fn read_placement_nbt(&self, nbt: &NbtCompound) {
        if let Some(position) = nbt.get_list("Pos")
            && position.len() >= 3
        {
            let x = position[0].extract_double().unwrap_or(0.0);
            let y = position[1].extract_double().unwrap_or(0.0);
            let z = position[2].extract_double().unwrap_or(0.0);
            let pos = Vector3::new(
                x.clamp(-MAX_HORIZONTAL_POSITION, MAX_HORIZONTAL_POSITION),
                y.clamp(-MAX_VERTICAL_POSITION, MAX_VERTICAL_POSITION),
                z.clamp(-MAX_HORIZONTAL_POSITION, MAX_HORIZONTAL_POSITION),
            );
            if pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() {
                self.set_pos(pos);
                self.last_sent_pos.store(pos);
            }
        }
        if let Some(velocity) = nbt.get_list("Motion")
            && velocity.len() >= 3
        {
            let [x, y, z] = [0, 1, 2].map(|axis| {
                let motion = velocity[axis].extract_double().unwrap_or(0.0);
                if motion.abs() > MAX_LOADED_MOTION {
                    0.0
                } else {
                    motion
                }
            });
            self.velocity.store(Vector3::new(x, y, z));
        }
        if let Some(rotation) = nbt.get_list("Rotation")
            && rotation.len() >= 2
        {
            let yaw = rotation[0].extract_float().unwrap_or(0.0);
            let pitch = rotation[1].extract_float().unwrap_or(0.0);
            if yaw.is_finite() && pitch.is_finite() {
                self.set_rotation(yaw, pitch);
                // TODO: use `pumpkin_util::math::pack_degrees`.
                let yaw_byte = (yaw * 256.0 / 360.0).rem_euclid(256.0) as u8;
                let pitch_byte = (pitch * 256.0 / 360.0).rem_euclid(256.0) as u8;
                self.last_sent_yaw.store(yaw_byte, Relaxed);
                self.last_sent_pitch.store(pitch_byte, Relaxed);
                self.head_yaw.store(yaw);
                self.last_sent_head_yaw.store(yaw_byte, Relaxed);
            }
        }
    }

    pub fn read_nbt_non_mut(&self, nbt: &NbtCompound) {
        self.read_placement_nbt(nbt);
        self.fire_ticks.store(
            i32::from(nbt.get_numeric_short("Fire").unwrap_or(0)),
            Relaxed,
        );
        self.on_ground
            .store(nbt.get_bool("OnGround").unwrap_or(false), Relaxed);
        if self.entity_type != &EntityType::PLAYER {
            let air = i32::from(
                nbt.get_numeric_short("Air")
                    .unwrap_or(breath::MAX_AIR as i16),
            );
            self.air_supply.store(air, Relaxed);
            self.set_synced_data(tracked_data::entity::DATA_AIR_SUPPLY_ID, VarInt(air));
        }
        self.invulnerable
            .store(nbt.get_bool("Invulnerable").unwrap_or(false), Relaxed);
        self.portal_cooldown
            .store(nbt.get_int("PortalCooldown").unwrap_or(0) as u32, Relaxed);
        if let Some(name) = nbt.get("CustomName") {
            // Vanilla stores a text component tag; a string is literal text. Older
            // Pumpkin saves hold a JSON string (same data version, so it can't be told
            // apart by version); the writer never emits a literal starting with '{'.
            let component = match name {
                NbtTag::String(json) if json.starts_with('{') => {
                    pumpkin_util::serde_json::from_str(json)
                        .unwrap_or_else(|_| TextComponent::from_nbt(name))
                }
                _ => TextComponent::from_nbt(name),
            };
            // set_custom_name also updates the synced tracked data
            self.set_custom_name(component);
        } else {
            self.clear_custom_name();
        }
        let custom_name_visible = nbt.get_bool("CustomNameVisible").unwrap_or(false);
        if self.custom_name_visible.load(Relaxed) != custom_name_visible {
            self.set_custom_name_visible(custom_name_visible);
        }
        self.set_silent(nbt.get_bool("Silent").unwrap_or(false));
        self.set_has_no_gravity(nbt.get_bool("NoGravity").unwrap_or(false));
        self.set_glowing(nbt.get_bool("Glowing").unwrap_or(false));
        self.frozen_ticks
            .store(nbt.get_int("TicksFrozen").unwrap_or(0), Relaxed);
        self.has_visual_fire
            .store(nbt.get_bool("HasVisualFire").unwrap_or(false), Relaxed);

        let custom_data = nbt
            .get_compound("data")
            .or_else(|| nbt.get_compound("PumpkinCustomData"))
            .or_else(|| nbt.get_compound("BukkitValues"));
        *self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            custom_data.cloned().unwrap_or_default();

        {
            let mut tags = self
                .scoreboard_tags
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tags.clear();
            if let Some(tag_list) = nbt.get_list("Tags") {
                tags.extend(
                    tag_list
                        .iter()
                        .filter_map(|tag| tag.extract_string().map(str::to_owned))
                        .take(MAX_SCOREBOARD_TAGS),
                );
            }
        }

        if let Some(team_name) = nbt.get_string("Team") {
            self.join_team(team_name);
        }
    }
}
