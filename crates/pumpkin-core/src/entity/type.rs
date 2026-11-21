#[cfg(test)]
mod golden;
pub mod spawn_rules;

use crate::entity::projectile::owner::ProjectileOwner;
use std::sync::Arc;

use pumpkin_data::entity::EntityType;
use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;

use crate::entity::boss::ender_dragon::EnderDragonEntity;
use crate::entity::boss::wither::WitherEntity;
use crate::entity::decoration::{
    armor_stand::ArmorStandEntity,
    cushion::CushionEntity,
    display::{BlockDisplayEntity, ItemDisplayEntity, TextDisplayEntity},
    end_crystal::EndCrystalEntity,
    item_frame::ItemFrameEntity,
    leash_knot::LeashKnotEntity,
    painting::PaintingEntity,
};
use crate::entity::experience_orb::ExperienceOrbEntity;
use crate::entity::falling::FallingEntity;
use crate::entity::interaction::InteractionEntity;
use crate::entity::item::ItemEntity;
use crate::entity::lightning::LightningBoltEntity;
use crate::entity::living::LivingEntity;
use crate::entity::marker::MarkerEntity;
use crate::entity::mob::bat::BatEntity;
use crate::entity::mob::blaze::BlazeEntity;
use crate::entity::mob::breeze::BreezeEntity;
use crate::entity::mob::cave_spider::CaveSpiderEntity;
use crate::entity::mob::creaking::CreakingEntity;
use crate::entity::mob::creeper::CreeperEntity;
use crate::entity::mob::elder_guardian::ElderGuardianEntity;
use crate::entity::mob::enderman::EndermanEntity;
use crate::entity::mob::endermite::EndermiteEntity;
use crate::entity::mob::evoker::EvokerEntity;
use crate::entity::mob::ghast::GhastEntity;
use crate::entity::mob::giant::GiantEntity;
use crate::entity::mob::guardian::GuardianEntity;
use crate::entity::mob::hoglin::HoglinEntity;
use crate::entity::mob::illusioner::IllusionerEntity;
use crate::entity::mob::magma_cube::MagmaCubeEntity;
use crate::entity::mob::phantom::PhantomEntity;
use crate::entity::mob::piglin::PiglinEntity;
use crate::entity::mob::piglin_brute::PiglinBruteEntity;
use crate::entity::mob::pillager::PillagerEntity;
use crate::entity::mob::ravager::RavagerEntity;
use crate::entity::mob::shulker::ShulkerEntity;
use crate::entity::mob::silverfish::SilverfishEntity;
use crate::entity::mob::skeleton::{
    bogged::BoggedSkeletonEntity, parched::ParchedSkeletonEntity, skeleton::SkeletonEntity,
    stray::StraySkeletonEntity, wither::WitherSkeletonEntity,
};
use crate::entity::mob::slime::SlimeEntity;
use crate::entity::mob::spider::SpiderEntity;
use crate::entity::mob::vex::VexEntity;
use crate::entity::mob::vindicator::VindicatorEntity;
use crate::entity::mob::warden::WardenEntity;
use crate::entity::mob::witch::WitchEntity;
use crate::entity::mob::zoglin::ZoglinEntity;
use crate::entity::mob::zombie::zombie_villager::ZombieVillagerEntity;
use crate::entity::mob::zombie::{drowned::DrownedEntity, husk::HuskEntity, zombie::ZombieEntity};
use crate::entity::mob::zombified_piglin::ZombifiedPiglinEntity;
use crate::entity::passive::allay::AllayEntity;
use crate::entity::passive::armadillo::ArmadilloEntity;
use crate::entity::passive::axolotl::AxolotlEntity;
use crate::entity::passive::bee::BeeEntity;
use crate::entity::passive::camel::CamelEntity;
use crate::entity::passive::cat::CatEntity;
use crate::entity::passive::chicken::ChickenEntity;
use crate::entity::passive::cod::CodEntity;
use crate::entity::passive::copper_golem::CopperGolemEntity;
use crate::entity::passive::cow::CowEntity;
use crate::entity::passive::dolphin::DolphinEntity;
use crate::entity::passive::donkey::DonkeyEntity;
use crate::entity::passive::fox::FoxEntity;
use crate::entity::passive::frog::FrogEntity;
use crate::entity::passive::glow_squid::GlowSquidEntity;
use crate::entity::passive::goat::GoatEntity;
use crate::entity::passive::happy_ghast::HappyGhastEntity;
use crate::entity::passive::horse::HorseEntity;
use crate::entity::passive::iron_golem::IronGolemEntity;
use crate::entity::passive::llama::LlamaEntity;
use crate::entity::passive::mooshroom::MooshroomEntity;
use crate::entity::passive::mule::MuleEntity;
use crate::entity::passive::nautilus::NautilusEntity;
use crate::entity::passive::ocelot::OcelotEntity;
use crate::entity::passive::panda::PandaEntity;
use crate::entity::passive::parrot::ParrotEntity;
use crate::entity::passive::pig::PigEntity;
use crate::entity::passive::polar_bear::PolarBearEntity;
use crate::entity::passive::pufferfish::PufferfishEntity;
use crate::entity::passive::rabbit::RabbitEntity;
use crate::entity::passive::salmon::SalmonEntity;
use crate::entity::passive::sheep::SheepEntity;
use crate::entity::passive::skeleton_horse::SkeletonHorseEntity;
use crate::entity::passive::sniffer::SnifferEntity;
use crate::entity::passive::snow_golem::SnowGolemEntity;
use crate::entity::passive::squid::SquidEntity;
use crate::entity::passive::strider::StriderEntity;
use crate::entity::passive::tadpole::TadpoleEntity;
use crate::entity::passive::trader_llama::TraderLlamaEntity;
use crate::entity::passive::tropical_fish::TropicalFishEntity;
use crate::entity::passive::turtle::TurtleEntity;
use crate::entity::passive::villager::VillagerEntity;
use crate::entity::passive::wandering_trader::WanderingTraderEntity;
use crate::entity::passive::wolf::WolfEntity;
use crate::entity::passive::zombie_horse::ZombieHorseEntity;
use crate::entity::projectile::ThrownItemEntity;
use crate::entity::projectile::arrow::ArrowEntity;
use crate::entity::projectile::egg::EggEntity;
use crate::entity::projectile::ender_pearl::EnderPearlEntity;
use crate::entity::projectile::experience_bottle::ExperienceBottleEntity;
use crate::entity::projectile::eye_of_ender::EyeOfEnder;
use crate::entity::projectile::fireball::FireballEntity;
use crate::entity::projectile::firework_rocket::FireworkRocketEntity;
use crate::entity::projectile::lingering_potion::LingeringPotionEntity;
use crate::entity::projectile::llama_spit::LlamaSpitEntity;
use crate::entity::projectile::shulker_bullet::ShulkerBulletEntity;
use crate::entity::projectile::small_fireball::SmallFireballEntity;
use crate::entity::projectile::snowball::SnowballEntity;
use crate::entity::projectile::splash_potion::SplashPotionEntity;
use crate::entity::projectile::trident::TridentEntity;
use crate::entity::projectile::wind_charge::{WIND_CHARGE_GRAVITY, WindChargeEntity};
use crate::entity::projectile::wither_skull::WitherSkullEntity;
use crate::entity::tnt::TNTEntity;
use crate::entity::vehicle::boat::BoatEntity;
use crate::entity::vehicle::minecart::MinecartEntity;
use crate::entity::{Entity, EntityBase};
use crate::world::World;
use pumpkin_data::Block;
use pumpkin_nbt::compound::NbtCompound;
use std::sync::atomic::AtomicBool;
use tracing::{debug, warn};

/// Builds the entity of one type from its base `Entity`.
type Constructor = fn(Entity) -> Arc<dyn EntityBase>;

/// Zombies, skeletons and the other hostile mobs.
fn hostile_mob(id: u16) -> Option<Constructor> {
    match id {
        id if id == EntityType::ZOMBIE.id => Some(|entity| ZombieEntity::new(entity)),
        id if id == EntityType::DROWNED.id => Some(|entity| DrownedEntity::new(entity)),
        id if id == EntityType::HUSK.id => Some(|entity| HuskEntity::new(entity)),
        id if id == EntityType::ZOMBIE_VILLAGER.id => {
            Some(|entity| ZombieVillagerEntity::new(entity))
        }
        id if id == EntityType::ZOMBIFIED_PIGLIN.id => {
            Some(|entity| ZombifiedPiglinEntity::new(entity))
        }
        id if id == EntityType::SKELETON.id => Some(|entity| SkeletonEntity::new(entity)),
        id if id == EntityType::BOGGED.id => Some(|entity| BoggedSkeletonEntity::new(entity)),
        id if id == EntityType::PARCHED.id => Some(|entity| ParchedSkeletonEntity::new(entity)),
        id if id == EntityType::WITHER_SKELETON.id => {
            Some(|entity| WitherSkeletonEntity::new(entity))
        }
        id if id == EntityType::STRAY.id => Some(|entity| StraySkeletonEntity::new(entity)),
        id if id == EntityType::BAT.id => Some(|entity| BatEntity::new(entity)),
        id if id == EntityType::CREEPER.id => Some(|entity| CreeperEntity::new(entity)),
        id if id == EntityType::ENDERMAN.id => Some(|entity| EndermanEntity::new(entity)),
        id if id == EntityType::BLAZE.id => Some(|entity| BlazeEntity::new(entity)),
        id if id == EntityType::SPIDER.id => Some(|entity| SpiderEntity::new(entity)),
        id if id == EntityType::CAVE_SPIDER.id => Some(|entity| CaveSpiderEntity::new(entity)),
        id if id == EntityType::GHAST.id => Some(|entity| GhastEntity::new(entity)),
        id if id == EntityType::MAGMA_CUBE.id => Some(|entity| MagmaCubeEntity::new(entity)),
        id if id == EntityType::PHANTOM.id => Some(|entity| PhantomEntity::new(entity)),
        id if id == EntityType::WITCH.id => Some(|entity| WitchEntity::new(entity)),
        id if id == EntityType::PIGLIN.id => Some(|entity| PiglinEntity::new(entity)),
        id if id == EntityType::PIGLIN_BRUTE.id => Some(|entity| PiglinBruteEntity::new(entity)),
        id if id == EntityType::PILLAGER.id => Some(|entity| PillagerEntity::new(entity)),
        id if id == EntityType::VINDICATOR.id => Some(|entity| VindicatorEntity::new(entity)),
        id if id == EntityType::EVOKER.id => Some(|entity| EvokerEntity::new(entity)),
        id if id == EntityType::RAVAGER.id => Some(|entity| RavagerEntity::new(entity)),
        id if id == EntityType::GUARDIAN.id => Some(|entity| GuardianEntity::new(entity)),
        id if id == EntityType::ELDER_GUARDIAN.id => {
            Some(|entity| ElderGuardianEntity::new(entity))
        }
        id if id == EntityType::WARDEN.id => Some(|entity| WardenEntity::new(entity)),
        id if id == EntityType::HOGLIN.id => Some(|entity| HoglinEntity::new(entity)),
        id if id == EntityType::ZOGLIN.id => Some(|entity| ZoglinEntity::new(entity)),
        id if id == EntityType::BREEZE.id => Some(|entity| BreezeEntity::new(entity)),
        id if id == EntityType::CREAKING.id => Some(|entity| CreakingEntity::new(entity)),
        id if id == EntityType::ILLUSIONER.id => Some(|entity| IllusionerEntity::new(entity)),
        id if id == EntityType::VEX.id => Some(|entity| VexEntity::new(entity)),
        id if id == EntityType::ENDERMITE.id => Some(|entity| EndermiteEntity::new(entity)),
        id if id == EntityType::GIANT.id => Some(|entity| GiantEntity::new(entity)),
        id if id == EntityType::SILVERFISH.id => Some(|entity| SilverfishEntity::new(entity)),
        id if id == EntityType::SLIME.id => Some(|entity| SlimeEntity::new(entity)),
        id if id == EntityType::SHULKER.id => Some(|entity| ShulkerEntity::new(entity)),
        _ => None,
    }
}

/// Animals, villagers, golems and the bosses.
fn passive_mob(id: u16) -> Option<Constructor> {
    match id {
        id if id == EntityType::CAT.id => Some(|entity| CatEntity::new(entity)),
        id if id == EntityType::CHICKEN.id => Some(|entity| ChickenEntity::new(entity)),
        id if id == EntityType::COW.id => Some(|entity| CowEntity::new(entity)),
        id if id == EntityType::PIG.id => Some(|entity| PigEntity::new(entity)),
        id if id == EntityType::SHEEP.id => Some(|entity| SheepEntity::new(entity)),
        id if id == EntityType::WOLF.id => Some(|entity| WolfEntity::new(entity)),
        id if id == EntityType::FOX.id => Some(|entity| FoxEntity::new(entity)),
        id if id == EntityType::RABBIT.id => Some(|entity| RabbitEntity::new(entity)),
        id if id == EntityType::TURTLE.id => Some(|entity| TurtleEntity::new(entity)),
        id if id == EntityType::VILLAGER.id => Some(|entity| VillagerEntity::new(entity)),
        id if id == EntityType::SQUID.id => Some(|entity| SquidEntity::new(entity)),
        id if id == EntityType::HORSE.id => Some(|entity| HorseEntity::new(entity)),
        id if id == EntityType::DONKEY.id => Some(|entity| DonkeyEntity::new(entity)),
        id if id == EntityType::MULE.id => Some(|entity| MuleEntity::new(entity)),
        id if id == EntityType::ZOMBIE_HORSE.id => Some(|entity| ZombieHorseEntity::new(entity)),
        id if id == EntityType::SKELETON_HORSE.id => {
            Some(|entity| SkeletonHorseEntity::new(entity))
        }
        id if id == EntityType::LLAMA.id => Some(|entity| LlamaEntity::new(entity)),
        id if id == EntityType::TRADER_LLAMA.id => Some(|entity| TraderLlamaEntity::new(entity)),
        id if id == EntityType::WANDERING_TRADER.id => {
            Some(|entity| WanderingTraderEntity::new(entity))
        }
        id if id == EntityType::ALLAY.id => Some(|entity| AllayEntity::new(entity)),
        id if id == EntityType::ARMADILLO.id => Some(|entity| ArmadilloEntity::new(entity)),
        id if id == EntityType::AXOLOTL.id => Some(|entity| AxolotlEntity::new(entity)),
        id if id == EntityType::BEE.id => Some(|entity| BeeEntity::new(entity)),
        id if id == EntityType::CAMEL.id => Some(|entity| CamelEntity::new(entity)),
        id if id == EntityType::COPPER_GOLEM.id => Some(|entity| CopperGolemEntity::new(entity)),
        id if id == EntityType::FROG.id => Some(|entity| FrogEntity::new(entity)),
        id if id == EntityType::GOAT.id => Some(|entity| GoatEntity::new(entity)),
        id if id == EntityType::HAPPY_GHAST.id => Some(|entity| HappyGhastEntity::new(entity)),
        id if id == EntityType::MOOSHROOM.id => Some(|entity| MooshroomEntity::new(entity)),
        id if id == EntityType::OCELOT.id => Some(|entity| OcelotEntity::new(entity)),
        id if id == EntityType::PANDA.id => Some(|entity| PandaEntity::new(entity)),
        id if id == EntityType::PARROT.id => Some(|entity| ParrotEntity::new(entity)),
        id if id == EntityType::POLAR_BEAR.id => Some(|entity| PolarBearEntity::new(entity)),
        id if id == EntityType::SNIFFER.id => Some(|entity| SnifferEntity::new(entity)),
        id if id == EntityType::STRIDER.id => Some(|entity| StriderEntity::new(entity)),
        id if id == EntityType::GLOW_SQUID.id => Some(|entity| GlowSquidEntity::new(entity)),
        id if id == EntityType::COD.id => Some(|entity| CodEntity::new(entity)),
        id if id == EntityType::SALMON.id => Some(|entity| SalmonEntity::new(entity)),
        id if id == EntityType::PUFFERFISH.id => Some(|entity| PufferfishEntity::new(entity)),
        id if id == EntityType::TROPICAL_FISH.id => Some(|entity| TropicalFishEntity::new(entity)),
        id if id == EntityType::TADPOLE.id => Some(|entity| TadpoleEntity::new(entity)),
        id if id == EntityType::DOLPHIN.id => Some(|entity| DolphinEntity::new(entity)),
        id if id == EntityType::NAUTILUS.id => Some(|entity| NautilusEntity::new(entity)),
        id if id == EntityType::SNOW_GOLEM.id => Some(|entity| SnowGolemEntity::new(entity)),
        id if id == EntityType::IRON_GOLEM.id => Some(|entity| IronGolemEntity::new(entity)),
        id if id == EntityType::WITHER.id => Some(|entity| WitherEntity::new(entity)),
        id if id == EntityType::ENDER_DRAGON.id => Some(|entity| EnderDragonEntity::new(entity)),
        _ => None,
    }
}

/// Armor stands, displays, frames, markers and the other non-mob entities that sit in the world.
fn decoration(id: u16) -> Option<Constructor> {
    match id {
        id if id == EntityType::AREA_EFFECT_CLOUD.id => {
            Some(|entity| crate::entity::area_effect_cloud::AreaEffectCloudEntity::new(entity))
        }
        id if id == EntityType::ARMOR_STAND.id => {
            Some(|entity| Arc::new(ArmorStandEntity::new(entity)))
        }
        id if id == EntityType::CUSHION.id => {
            Some(|entity| Arc::new(CushionEntity::new(entity, 0)))
        }
        id if id == EntityType::BLOCK_DISPLAY.id => Some(|entity| BlockDisplayEntity::new(entity)),
        id if id == EntityType::ITEM_DISPLAY.id => Some(|entity| ItemDisplayEntity::new(entity)),
        id if id == EntityType::TEXT_DISPLAY.id => Some(|entity| TextDisplayEntity::new(entity)),
        id if id == EntityType::PAINTING.id => Some(|entity| Arc::new(PaintingEntity::new(entity))),
        id if id == EntityType::LEASH_KNOT.id => {
            Some(|entity| Arc::new(LeashKnotEntity::new(entity)))
        }
        id if id == EntityType::ITEM_FRAME.id || id == EntityType::GLOW_ITEM_FRAME.id => {
            Some(|entity| Arc::new(ItemFrameEntity::new(entity)))
        }
        id if id == EntityType::END_CRYSTAL.id => {
            Some(|entity| Arc::new(EndCrystalEntity::new(entity)))
        }
        id if id == EntityType::LIGHTNING_BOLT.id => {
            Some(|entity| Arc::new(LightningBoltEntity::new(entity)))
        }
        id if id == EntityType::MARKER.id => Some(|entity| MarkerEntity::new(entity)),
        id if id == EntityType::INTERACTION.id => Some(|entity| InteractionEntity::new(entity)),
        _ => None,
    }
}

/// Projectiles.
fn projectile(id: u16) -> Option<Constructor> {
    match id {
        id if id == EntityType::EVOKER_FANGS.id => Some(|entity| {
            Arc::new(
                crate::entity::projectile::evoker_fangs::EvokerFangsEntity::new(
                    entity, 0, 0.0, None,
                ),
            )
        }),
        id if id == EntityType::ENDER_PEARL.id => {
            Some(|entity| Arc::new(EnderPearlEntity::new(entity)))
        }
        id if id == EntityType::SNOWBALL.id => Some(|entity| Arc::new(SnowballEntity::new(entity))),
        id if id == EntityType::EGG.id => Some(|entity| Arc::new(EggEntity::new(entity))),
        id if id == EntityType::SHULKER_BULLET.id => Some(|entity| {
            // Shulker bullets are normally spawned by ShulkerEntity directly;
            // when loaded from the world we create a no-target bullet at the given position.
            Arc::new(ShulkerBulletEntity::orphan(entity))
        }),
        id if id == EntityType::ARROW.id => Some(|entity| Arc::new(ArrowEntity::new(entity, None))),
        id if id == EntityType::SPECTRAL_ARROW.id => {
            Some(|entity| Arc::new(ArrowEntity::new(entity, None)))
        }
        id if id == EntityType::TRIDENT.id => {
            Some(|entity| Arc::new(TridentEntity::new(entity, None)))
        }
        id if id == EntityType::FIREBALL.id => Some(|entity| Arc::new(FireballEntity::new(entity))),
        id if id == EntityType::SMALL_FIREBALL.id => {
            Some(|entity| Arc::new(SmallFireballEntity::new(entity)))
        }
        id if id == EntityType::WITHER_SKULL.id => {
            Some(|entity| Arc::new(WitherSkullEntity::new(entity)))
        }
        id if id == EntityType::WIND_CHARGE.id => Some(|entity| {
            let thrown = ThrownItemEntity {
                entity,
                owner: ProjectileOwner::none(),
                collides_with_projectiles: false,
                has_hit: AtomicBool::new(false),
                gravity: WIND_CHARGE_GRAVITY,
            };
            Arc::new(WindChargeEntity::new_normal(thrown))
        }),
        id if id == EntityType::BREEZE_WIND_CHARGE.id => Some(|entity| {
            let thrown = ThrownItemEntity {
                entity,
                owner: ProjectileOwner::none(),
                collides_with_projectiles: false,
                has_hit: AtomicBool::new(false),
                gravity: WIND_CHARGE_GRAVITY,
            };
            Arc::new(WindChargeEntity::new_breeze(thrown))
        }),
        id if id == EntityType::FIREWORK_ROCKET.id => {
            Some(|entity| Arc::new(FireworkRocketEntity::new(entity)))
        }
        id if id == EntityType::SPLASH_POTION.id => {
            Some(|entity| Arc::new(SplashPotionEntity::new(entity)))
        }
        id if id == EntityType::EXPERIENCE_BOTTLE.id => {
            Some(|entity| Arc::new(ExperienceBottleEntity::new(entity)))
        }
        id if id == EntityType::LINGERING_POTION.id => {
            Some(|entity| Arc::new(LingeringPotionEntity::new(entity)))
        }
        id if id == EntityType::LLAMA_SPIT.id => {
            Some(|entity| Arc::new(LlamaSpitEntity::new(entity)))
        }
        id if id == EntityType::EYE_OF_ENDER.id => Some(|entity| Arc::new(EyeOfEnder::new(entity))),
        _ => None,
    }
}

/// Falling blocks, experience orbs, tnt, items, minecarts and boats.
fn other_entity(id: u16) -> Option<Constructor> {
    match id {
        id if id == EntityType::FALLING_BLOCK.id => {
            Some(|entity| Arc::new(FallingEntity::new(entity, Block::SAND.default_state.id)))
        }
        id if id == EntityType::EXPERIENCE_ORB.id => {
            Some(|entity| Arc::new(ExperienceOrbEntity::new(entity, 1)))
        }
        id if id == EntityType::TNT.id => Some(|entity| {
            Arc::new(TNTEntity::new(
                entity,
                TNTEntity::DEFAULT_POWER,
                TNTEntity::DEFAULT_FUSE,
            ))
        }),
        id if id == EntityType::ITEM.id => Some(|entity| Arc::new(ItemEntity::new_empty(entity))),
        id if id == EntityType::MINECART.id
            || id == EntityType::CHEST_MINECART.id
            || id == EntityType::FURNACE_MINECART.id
            || id == EntityType::TNT_MINECART.id
            || id == EntityType::HOPPER_MINECART.id
            || id == EntityType::COMMAND_BLOCK_MINECART.id
            || id == EntityType::SPAWNER_MINECART.id =>
        {
            Some(|entity| Arc::new(MinecartEntity::new(entity)))
        }
        id if id == EntityType::ACACIA_BOAT.id
            || id == EntityType::ACACIA_CHEST_BOAT.id
            || id == EntityType::BIRCH_BOAT.id
            || id == EntityType::BIRCH_CHEST_BOAT.id
            || id == EntityType::DARK_OAK_BOAT.id
            || id == EntityType::DARK_OAK_CHEST_BOAT.id
            || id == EntityType::JUNGLE_BOAT.id
            || id == EntityType::JUNGLE_CHEST_BOAT.id
            || id == EntityType::MANGROVE_BOAT.id
            || id == EntityType::MANGROVE_CHEST_BOAT.id
            || id == EntityType::OAK_BOAT.id
            || id == EntityType::OAK_CHEST_BOAT.id
            || id == EntityType::PALE_OAK_BOAT.id
            || id == EntityType::PALE_OAK_CHEST_BOAT.id
            || id == EntityType::SPRUCE_BOAT.id
            || id == EntityType::SPRUCE_CHEST_BOAT.id
            || id == EntityType::BAMBOO_RAFT.id
            || id == EntityType::BAMBOO_CHEST_RAFT.id
            || id == EntityType::CHERRY_BOAT.id
            || id == EntityType::CHERRY_CHEST_BOAT.id
            || id == EntityType::POPLAR_BOAT.id
            || id == EntityType::POPLAR_CHEST_BOAT.id =>
        {
            Some(|entity| Arc::new(BoatEntity::new(entity)))
        }
        _ => None,
    }
}

pub fn from_type(
    entity_type: &'static EntityType,
    position: Vector3<f64>,
    world: &Arc<World>,
    uuid: Uuid,
) -> Arc<dyn EntityBase> {
    let entity = Entity::from_uuid(uuid, world.clone(), position, entity_type);
    let id = entity_type.id;
    let constructor = hostile_mob(id)
        .or_else(|| passive_mob(id))
        .or_else(|| decoration(id))
        .or_else(|| projectile(id))
        .or_else(|| other_entity(id));
    if let Some(constructor) = constructor {
        return constructor(entity);
    }
    // Fallback Entity
    if entity_type.attributes.is_empty() {
        Arc::new(entity)
    } else {
        Arc::new(LivingEntity::new(entity))
    }
}

fn create_entity(
    entity_type: &'static EntityType,
    position: Vector3<f64>,
    nbt: &NbtCompound,
    world: &Arc<World>,
) -> Arc<dyn EntityBase> {
    let uuid = nbt.get_uuid("UUID").unwrap_or_else(Uuid::new_v4);
    let entity = from_type(entity_type, position, world, uuid);
    entity.read_nbt_non_mut(nbt);
    entity
}

fn entity_type_from_nbt(nbt: &NbtCompound) -> Option<&'static EntityType> {
    let Some(id) = nbt.get_string("id") else {
        debug!("Entity has no ID");
        return None;
    };
    let entity_type = EntityType::from_name(id.strip_prefix("minecraft:").unwrap_or(id));
    if entity_type.is_none() {
        warn!("Entity has no valid Entity Type {id}");
    }
    entity_type
}

pub fn load_entity(
    entity_type: &'static EntityType,
    position: Vector3<f64>,
    nbt: &NbtCompound,
    world: &Arc<World>,
) -> Arc<dyn EntityBase> {
    let entity = create_entity(entity_type, position, nbt, world);
    for passenger in nbt.get_list("Passengers").unwrap_or_default() {
        if let Some(passenger) = passenger.extract_compound().and_then(|passenger| {
            load_entity_from_nbt(passenger, entity.get_entity().pos.load(), world)
        }) {
            entity
                .get_entity()
                .add_passenger_silently(entity.clone(), passenger);
        }
    }
    entity
}

pub fn load_entity_from_nbt(
    nbt: &NbtCompound,
    position: Vector3<f64>,
    world: &Arc<World>,
) -> Option<Arc<dyn EntityBase>> {
    Some(load_entity(
        entity_type_from_nbt(nbt)?,
        position,
        nbt,
        world,
    ))
}

pub fn load_static_entity_from_nbt(
    nbt: &NbtCompound,
    position: Vector3<f64>,
    world: &Arc<World>,
) -> Option<Arc<dyn EntityBase>> {
    Some(create_entity(
        entity_type_from_nbt(nbt)?,
        position,
        nbt,
        world,
    ))
}

#[must_use]
pub fn self_and_passengers(entity: &Arc<dyn EntityBase>) -> Vec<Arc<dyn EntityBase>> {
    let mut stack = vec![entity.clone()];
    let passengers = entity
        .get_entity()
        .passengers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    for passenger in &passengers {
        stack.extend(self_and_passengers(passenger));
    }
    stack
}
