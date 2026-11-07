//! Where an entity type may spawn naturally.
//!
//! Which rule an entity type follows is a table, so the rule of every entity type can be listed
//! and tested without a world; what each rule checks is a function of its own.

use pumpkin_data::entity::EntityType;
use pumpkin_data::tag::Taggable;
use pumpkin_util::math::position::BlockPos;

use crate::entity::mob::{self, bat::BatEntity, slime::SlimeEntity};
use crate::world::World;

/// How the natural spawner decides whether an entity type may spawn at a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpawnRule {
    Monster,
    AnyLightMonster,
    SurfaceMonster,
    Stray,
    Pillager,
    SilverfishLike,
    Ghast,
    MagmaCube,
    Always,
    Slime,
    HoglinOrPiglin,
    ZombifiedPiglin,
    Strider,
    Bat,
    Animal,
    Armadillo,
    Camel,
    Frog,
    Goat,
    Mooshroom,
    Parrot,
    Rabbit,
    Wolf,
    Fox,
    Ocelot,
    PolarBear,
    Turtle,
    SurfaceWaterAnimal,
    SurfaceAgeableWaterCreature,
    TropicalFish,
    Axolotl,
    GlowSquid,
    Nautilus,
    Drowned,
    Guardian,
    GenericMob,
}

const RULES: &[(SpawnRule, &[u16])] = &[
    (
        SpawnRule::Monster,
        &[
            EntityType::BOGGED.id,
            EntityType::CAVE_SPIDER.id,
            EntityType::CREEPER.id,
            EntityType::ENDERMAN.id,
            EntityType::GIANT.id,
            EntityType::RAVAGER.id,
            EntityType::SKELETON.id,
            EntityType::SPIDER.id,
            EntityType::WITCH.id,
            EntityType::WITHER.id,
            EntityType::WITHER_SKELETON.id,
            EntityType::ZOMBIE.id,
            EntityType::ZOMBIE_HORSE.id,
            EntityType::ZOMBIE_VILLAGER.id,
            EntityType::CREAKING.id,
            EntityType::EVOKER.id,
            EntityType::ILLUSIONER.id,
            EntityType::VEX.id,
            EntityType::VINDICATOR.id,
            EntityType::WARDEN.id,
        ],
    ),
    (
        SpawnRule::AnyLightMonster,
        &[
            EntityType::BLAZE.id,
            EntityType::BREEZE.id,
            EntityType::ZOGLIN.id,
        ],
    ),
    (
        SpawnRule::SurfaceMonster,
        &[
            EntityType::HUSK.id,
            EntityType::PARCHED.id,
            EntityType::CAMEL_HUSK.id,
        ],
    ),
    (SpawnRule::Stray, &[EntityType::STRAY.id]),
    (SpawnRule::Pillager, &[EntityType::PILLAGER.id]),
    (
        SpawnRule::SilverfishLike,
        &[EntityType::ENDERMITE.id, EntityType::SILVERFISH.id],
    ),
    (SpawnRule::Ghast, &[EntityType::GHAST.id]),
    (SpawnRule::MagmaCube, &[EntityType::MAGMA_CUBE.id]),
    (SpawnRule::Always, &[EntityType::SULFUR_CUBE.id]),
    (SpawnRule::Slime, &[EntityType::SLIME.id]),
    (
        SpawnRule::HoglinOrPiglin,
        &[EntityType::HOGLIN.id, EntityType::PIGLIN.id],
    ),
    (
        SpawnRule::ZombifiedPiglin,
        &[EntityType::ZOMBIFIED_PIGLIN.id],
    ),
    (SpawnRule::Strider, &[EntityType::STRIDER.id]),
    (SpawnRule::Bat, &[EntityType::BAT.id]),
    (
        SpawnRule::Animal,
        &[
            EntityType::CHICKEN.id,
            EntityType::COW.id,
            EntityType::DONKEY.id,
            EntityType::HAPPY_GHAST.id,
            EntityType::HORSE.id,
            EntityType::LLAMA.id,
            EntityType::MULE.id,
            EntityType::PIG.id,
            EntityType::SHEEP.id,
            EntityType::CAT.id,
            EntityType::PANDA.id,
            EntityType::TRADER_LLAMA.id,
            EntityType::SKELETON_HORSE.id,
        ],
    ),
    (SpawnRule::Armadillo, &[EntityType::ARMADILLO.id]),
    (SpawnRule::Camel, &[EntityType::CAMEL.id]),
    (SpawnRule::Frog, &[EntityType::FROG.id]),
    (SpawnRule::Goat, &[EntityType::GOAT.id]),
    (SpawnRule::Mooshroom, &[EntityType::MOOSHROOM.id]),
    (SpawnRule::Parrot, &[EntityType::PARROT.id]),
    (SpawnRule::Rabbit, &[EntityType::RABBIT.id]),
    (SpawnRule::Wolf, &[EntityType::WOLF.id]),
    (SpawnRule::Fox, &[EntityType::FOX.id]),
    (SpawnRule::Ocelot, &[EntityType::OCELOT.id]),
    (SpawnRule::PolarBear, &[EntityType::POLAR_BEAR.id]),
    (SpawnRule::Turtle, &[EntityType::TURTLE.id]),
    (
        SpawnRule::SurfaceWaterAnimal,
        &[
            EntityType::COD.id,
            EntityType::PUFFERFISH.id,
            EntityType::SALMON.id,
        ],
    ),
    (
        SpawnRule::SurfaceAgeableWaterCreature,
        &[EntityType::DOLPHIN.id, EntityType::SQUID.id],
    ),
    (SpawnRule::TropicalFish, &[EntityType::TROPICAL_FISH.id]),
    (SpawnRule::Axolotl, &[EntityType::AXOLOTL.id]),
    (SpawnRule::GlowSquid, &[EntityType::GLOW_SQUID.id]),
    (SpawnRule::Nautilus, &[EntityType::NAUTILUS.id]),
    (SpawnRule::Drowned, &[EntityType::DROWNED.id]),
    (
        SpawnRule::Guardian,
        &[EntityType::GUARDIAN.id, EntityType::ELDER_GUARDIAN.id],
    ),
];

/// The rule `entity_type` follows. Entity types in no row of [`RULES`] are checked as plain mobs
/// when they are mobs and may spawn anywhere otherwise.
fn spawn_rule(entity_type: &'static EntityType) -> SpawnRule {
    RULES
        .iter()
        .find(|(_, ids)| ids.contains(&entity_type.id))
        .map_or_else(
            || {
                if entity_type.mob {
                    SpawnRule::GenericMob
                } else {
                    SpawnRule::Always
                }
            },
            |(rule, _)| *rule,
        )
}

pub fn check_spawn_rules(
    entity_type: &'static EntityType,
    world: &World,
    pos: &BlockPos,
    is_thundering: bool,
) -> bool {
    match spawn_rule(entity_type) {
        SpawnRule::Monster => mob::MobEntity::check_monster_spawn_rules(world, pos, is_thundering),
        SpawnRule::AnyLightMonster => {
            mob::MobEntity::check_any_light_monster_spawn_rules(world, pos)
        }
        SpawnRule::SurfaceMonster => {
            mob::MobEntity::check_surface_monsters_spawn_rules(world, pos, is_thundering)
        }
        SpawnRule::Stray => stray(world, pos, is_thundering),
        SpawnRule::Pillager => pillager(world, pos),
        SpawnRule::SilverfishLike => silverfish_like(world, pos),
        SpawnRule::Ghast => ghast(world, pos),
        SpawnRule::MagmaCube => magma_cube(world, pos),
        SpawnRule::Always => true,
        SpawnRule::Slime => SlimeEntity::check_slime_spawn_rules(world, pos),
        SpawnRule::HoglinOrPiglin => hoglin_or_piglin(world, pos),
        SpawnRule::ZombifiedPiglin => zombified_piglin(world, pos),
        SpawnRule::Strider => strider(world, pos),
        SpawnRule::Bat => BatEntity::check_bat_spawn_rules(world, pos),
        SpawnRule::Animal => mob::MobEntity::check_animal_spawn_rules(world, pos),
        SpawnRule::Armadillo => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_ARMADILLO_SPAWNABLE_ON,
        ),
        SpawnRule::Camel => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_CAMELS_SPAWNABLE_ON,
        ),
        SpawnRule::Frog => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_FROGS_SPAWNABLE_ON,
        ),
        SpawnRule::Goat => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_GOATS_SPAWNABLE_ON,
        ),
        SpawnRule::Mooshroom => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_MOOSHROOMS_SPAWNABLE_ON,
        ),
        SpawnRule::Parrot => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_PARROTS_SPAWNABLE_ON,
        ),
        SpawnRule::Rabbit => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_RABBITS_SPAWNABLE_ON,
        ),
        SpawnRule::Wolf => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_WOLVES_SPAWNABLE_ON,
        ),
        SpawnRule::Fox => spawns_on_in_light(
            world,
            pos,
            &pumpkin_data::tag::Block::MINECRAFT_FOXES_SPAWNABLE_ON,
        ),
        SpawnRule::Ocelot => ocelot(world, pos),
        SpawnRule::PolarBear => polar_bear(world, pos),
        SpawnRule::Turtle => turtle(world, pos),
        SpawnRule::SurfaceWaterAnimal => {
            mob::MobEntity::check_surface_water_animal_spawn_rules(world, pos)
        }
        SpawnRule::SurfaceAgeableWaterCreature => {
            mob::MobEntity::check_surface_ageable_water_creature_spawn_rules(world, pos)
        }
        SpawnRule::TropicalFish => tropical_fish(world, pos),
        SpawnRule::Axolotl => world
            .get_block(&pos.down())
            .has_tag(&pumpkin_data::tag::Block::MINECRAFT_AXOLOTLS_SPAWNABLE_ON),
        SpawnRule::GlowSquid => glow_squid(world, pos),
        SpawnRule::Nautilus => nautilus(world, pos),
        SpawnRule::Drowned => drowned(world, pos, is_thundering),
        SpawnRule::Guardian => guardian(world, pos),
        SpawnRule::GenericMob => mob::MobEntity::check_mob_spawn_rules(world, pos),
    }
}

fn is_peaceful(world: &World) -> bool {
    world.level_info.load().difficulty == pumpkin_util::Difficulty::Peaceful
}

fn in_water(world: &World, pos: &BlockPos) -> bool {
    world
        .get_fluid(&pos.down())
        .has_tag(&pumpkin_data::tag::Fluid::MINECRAFT_WATER)
        && world
            .get_fluid(pos)
            .has_tag(&pumpkin_data::tag::Fluid::MINECRAFT_WATER)
}

/// The block below carries `tag` and the light is bright enough.
fn spawns_on_in_light(world: &World, pos: &BlockPos, tag: &'static pumpkin_data::tag::Tag) -> bool {
    let below = pos.down();
    world.get_block(&below).has_tag(tag) && mob::MobEntity::is_bright_enough_to_spawn(world, pos)
}

fn stray(world: &World, pos: &BlockPos, is_thundering: bool) -> bool {
    if !mob::MobEntity::check_monster_spawn_rules(world, pos, is_thundering) {
        return false;
    }
    let mut check_sky_pos = *pos;
    while world.get_block(&check_sky_pos) == &pumpkin_data::Block::POWDER_SNOW {
        check_sky_pos = check_sky_pos.up();
    }
    world.can_see_sky(&check_sky_pos.down())
}

fn pillager(world: &World, pos: &BlockPos) -> bool {
    if world.get_block_light_level(pos).unwrap_or(0) > 8 {
        return false;
    }
    mob::MobEntity::check_any_light_monster_spawn_rules(world, pos)
}

/// Endermite and silverfish: not within five blocks of a player.
fn silverfish_like(world: &World, pos: &BlockPos) -> bool {
    if !mob::MobEntity::check_any_light_monster_spawn_rules(world, pos) {
        return false;
    }
    world
        .get_closest_player(pos.to_centered_f64(), 5.0)
        .is_none()
}

fn ghast(world: &World, pos: &BlockPos) -> bool {
    if is_peaceful(world) {
        return false;
    }
    if rand::random_range(0..20) != 0 {
        return false;
    }
    mob::MobEntity::check_mob_spawn_rules(world, pos)
}

fn magma_cube(world: &World, pos: &BlockPos) -> bool {
    if is_peaceful(world) {
        return false;
    }
    mob::MobEntity::check_mob_spawn_rules(world, pos)
}

fn hoglin_or_piglin(world: &World, pos: &BlockPos) -> bool {
    let below = pos.down();
    if world.get_block(&below) == &pumpkin_data::Block::NETHER_WART_BLOCK {
        return false;
    }
    mob::MobEntity::check_mob_spawn_rules(world, pos)
}

fn zombified_piglin(world: &World, pos: &BlockPos) -> bool {
    if is_peaceful(world) {
        return false;
    }
    let below = pos.down();
    if world.get_block(&below) == &pumpkin_data::Block::NETHER_WART_BLOCK {
        return false;
    }
    mob::MobEntity::check_mob_spawn_rules(world, pos)
}

fn strider(world: &World, pos: &BlockPos) -> bool {
    let mut check_pos = *pos;
    while world
        .get_fluid(&check_pos)
        .has_tag(&pumpkin_data::tag::Fluid::MINECRAFT_LAVA)
    {
        check_pos = check_pos.up();
    }
    world.get_block(&check_pos).is_air()
}

fn ocelot(world: &World, pos: &BlockPos) -> bool {
    if rand::random_range(0..3) == 0 {
        return false;
    }
    mob::MobEntity::check_mob_spawn_rules(world, pos)
}

fn polar_bear(world: &World, pos: &BlockPos) -> bool {
    let below = pos.down();
    let biome = world.get_biome(pos);
    if biome
        .has_tag(&pumpkin_data::tag::WorldgenBiome::MINECRAFT_POLAR_BEARS_SPAWN_ON_ALTERNATE_BLOCKS)
    {
        return mob::MobEntity::is_bright_enough_to_spawn(world, pos)
            && world
                .get_block(&below)
                .has_tag(&pumpkin_data::tag::Block::MINECRAFT_POLAR_BEARS_SPAWNABLE_ON_ALTERNATE);
    }
    mob::MobEntity::check_animal_spawn_rules(world, pos)
}

fn turtle(world: &World, pos: &BlockPos) -> bool {
    let sea_level = world.sea_level;
    let below = pos.down();
    pos.0.y < sea_level + 4
        && world
            .get_block(&below)
            .has_tag(&pumpkin_data::tag::Block::MINECRAFT_SAND)
        && mob::MobEntity::is_bright_enough_to_spawn(world, pos)
}

fn tropical_fish(world: &World, pos: &BlockPos) -> bool {
    let biome = world.get_biome(pos);
    let valid_water = world
        .get_fluid(&pos.down())
        .has_tag(&pumpkin_data::tag::Fluid::MINECRAFT_WATER)
        && world.get_block(&pos.up()) == &pumpkin_data::Block::WATER;
    if !valid_water {
        return false;
    }
    biome.has_tag(
        &pumpkin_data::tag::WorldgenBiome::MINECRAFT_ALLOWS_TROPICAL_FISH_SPAWNS_AT_ANY_HEIGHT,
    ) || mob::MobEntity::check_surface_water_animal_spawn_rules(world, pos)
}

fn glow_squid(world: &World, pos: &BlockPos) -> bool {
    let sea_level = world.sea_level;
    pos.0.y <= sea_level - 33
        && world.get_raw_brightness(pos, 0) == 0
        && world.get_block(pos) == &pumpkin_data::Block::WATER
}

fn nautilus(world: &World, pos: &BlockPos) -> bool {
    let sea_level = world.sea_level;
    let min_spawn_level = sea_level - 25;
    pos.0.y >= min_spawn_level
        && pos.0.y <= sea_level - 5
        && world
            .get_fluid(&pos.down())
            .has_tag(&pumpkin_data::tag::Fluid::MINECRAFT_WATER)
        && (world.get_block(&pos.up()) == &pumpkin_data::Block::WATER
            || world
                .get_fluid(&pos.up())
                .has_tag(&pumpkin_data::tag::Fluid::MINECRAFT_WATER))
}

fn drowned(world: &World, pos: &BlockPos, is_thundering: bool) -> bool {
    if is_peaceful(world) {
        return false;
    }
    if !in_water(world, pos) {
        return false;
    }
    if !mob::MobEntity::is_dark_enough_to_spawn(world, pos, is_thundering) {
        return false;
    }
    let biome = world.get_biome(pos);
    if biome.has_tag(&pumpkin_data::tag::WorldgenBiome::MINECRAFT_MORE_FREQUENT_DROWNED_SPAWNS) {
        return rand::random_range(0..15) == 0;
    }
    let sea_level = world.sea_level;
    pos.0.y < sea_level - 5 && rand::random_range(0..40) == 0
}

fn guardian(world: &World, pos: &BlockPos) -> bool {
    if is_peaceful(world) {
        return false;
    }
    if !in_water(world, pos) {
        return false;
    }
    rand::random_range(0..20) == 0 || !world.can_see_sky(pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entity_type_keeps_its_spawn_rule() -> Result<(), Box<dyn std::error::Error>> {
        use std::fmt::Write;
        let mut now = String::new();
        for entity_type in (0..2000u16).filter_map(EntityType::from_raw) {
            writeln!(
                now,
                "{} {:?}",
                entity_type.resource_name,
                spawn_rule(entity_type)
            )?;
        }
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/spawn_rule.txt");
        assert_eq!(std::fs::read_to_string(path)?, now);
        Ok(())
    }
}
