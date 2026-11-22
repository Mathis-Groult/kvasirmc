//! A record of what `DispenserBlock::dispense` does, made before the function was split
//! (`tests/golden/dispenser_dispense.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! One scene per behaviour of the vanilla `DispenseItemBehavior` registry (every item class the
//! chain routes: arrows, boats, minecarts, armor stands, TNT, thrown items, fire and wind
//! charges, fireworks, buckets, flint and steel, honeycombs, spawn eggs, shears, glass and water
//! bottles, glowstone, mob heads, shulker boxes, equipment, experience bottles and the default
//! drop), each with the successful and the refused outcome where the chain tells them apart,
//! each run in front of a real dispenser facing north, east, south, west, up and down. A
//! listener player in the world hears the world events and sounds; a spy plugin cancels the
//! `BlockDispenseEvent` in the cancelled scenes and logs the events and game events. One line
//! per scene and facing: the stack left, the dispenser slots, the packets heard, the event log,
//! the entities spawned (type, position, flight axis), the entities that stood in front (their
//! equipment, sheared state) and every block that changed around the dispenser.
//!
//! Not seen: hands (a spawned zombie may hold a random tool), projectile speed and spread and the sheep wool count (random), packets other than
//! world events and sounds.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::num::NonZero;
use std::sync::{Arc, Mutex, PoisonError};

use pumpkin_data::block_properties::{
    BeeNestLikeProperties, DispenserLikeProperties, Facing, RailLikeProperties, RailShape,
    RespawnAnchorLikeProperties,
};
use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{EquipmentSlot, PotionContentsImpl};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Potion;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::dispenser::{DispenseContext, DispenserBlock};
#[path = "golden_text.rs"]
mod text;
use crate::block::entities::dispenser::DispenserBlockEntity;
use crate::block::{BlockBehaviour, OnScheduledTickArgs};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::entity::passive::sheep::SheepEntity;
use crate::entity::player::Player;
use crate::entity::r#type::from_type;
use crate::net::java::PacketTap;
use crate::plugin::api::events::block::block_dispense::BlockDispenseEvent;
use crate::plugin::api::events::world::generic_game::GenericGameEvent;
use crate::plugin::api::events::{Cancellable, EventPriority};
use crate::plugin::{BoxFuture, EventHandler};
use crate::server::Server;
use crate::world::World;
use text::{changes_text, entity_text, equipment_text, heard, region, slots_text, stack_text, };

const FACINGS: [Facing; 6] = [
    Facing::North,
    Facing::East,
    Facing::South,
    Facing::West,
    Facing::Up,
    Facing::Down,
];

/// Scenes per row of the grid, and the distance between two scenes.
const COLUMNS: usize = 16;
const SPACING: i32 = 8;

/// The block the dispenser stands on and the block in front of it, with helpers that place
/// blocks and mobs around them. Nothing is ever placed over the dispenser itself.
struct Site<'a> {
    world: &'a Arc<World>,
    pos: BlockPos,
    front: BlockPos,
}

impl Site<'_> {
    fn put(&self, at: BlockPos, state: BlockStateId) {
        if at != self.pos {
            self.world.set_block_state(
                &at,
                state,
                BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
            );
        }
    }

    fn block(&self, at: BlockPos, block: &Block) {
        self.put(at, block.default_state.id);
    }

    fn mob(&self, kind: &'static EntityType) -> Arc<dyn EntityBase> {
        let at = Vector3::new(
            f64::from(self.front.0.x) + 0.5,
            f64::from(self.front.0.y),
            f64::from(self.front.0.z) + 0.5,
        );
        let mob = from_type(kind, at, self.world, Uuid::new_v4());
        self.world.spawn_entity(mob.clone());
        mob
    }
}

type Setup = fn(&Site<'_>);

/// One scene of the corpus.
struct Scene {
    name: &'static str,
    item: ItemStack,
    setup: Setup,
    cancelled: bool,
    full: bool,
    nether: bool,
    tnt_off: bool,
    loose: bool,
    player_in_front: bool,
    ticked: bool,
}

impl Scene {
    fn new(name: &'static str, item: ItemStack) -> Self {
        Self {
            name,
            item,
            setup: nothing,
            cancelled: false,
            full: false,
            nether: false,
            tnt_off: false,
            loose: false,
            player_in_front: false,
            ticked: false,
        }
    }

    fn with(mut self, setup: Setup) -> Self {
        self.setup = setup;
        self
    }

    fn cancelled(mut self) -> Self {
        self.cancelled = true;
        self
    }

    /// Every other slot of the dispenser holds cobblestone.
    fn full(mut self) -> Self {
        self.full = true;
        self
    }

    fn nether(mut self) -> Self {
        self.nether = true;
        self
    }

    fn tnt_off(mut self) -> Self {
        self.tnt_off = true;
        self
    }

    /// Item counts are not recorded (a sheep drops 1 to 3 wool).
    fn loose(mut self) -> Self {
        self.loose = true;
        self
    }

    fn player_in_front(mut self) -> Self {
        self.player_in_front = true;
        self
    }

    /// Goes through the block's scheduled tick (random slot, game events) instead of `dispense`.
    fn ticked(mut self) -> Self {
        self.ticked = true;
        self
    }
}

fn item(which: &'static Item, count: u8) -> ItemStack {
    ItemStack::new(count, which)
}

fn potion_of(potion: &Potion) -> ItemStack {
    ItemStack::new_with_component(
        1,
        &Item::POTION,
        vec![(
            DataComponent::PotionContents,
            Some(Box::new(PotionContentsImpl {
                potion_id: Some(i32::from(potion.id)),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            }) as Box<_>),
        )],
    )
}

fn nothing(_: &Site<'_>) {}

fn water_front(site: &Site<'_>) {
    site.block(site.front, &Block::WATER);
}

fn water_below_front(site: &Site<'_>) {
    site.block(site.front.down(), &Block::WATER);
}

fn crowded_water(site: &Site<'_>) {
    water_front(site);
    site.mob(&EntityType::ARMOR_STAND);
}

fn lava_front(site: &Site<'_>) {
    site.block(site.front, &Block::LAVA);
}

fn stone_front(site: &Site<'_>) {
    site.block(site.front, &Block::STONE);
}

fn dirt_front(site: &Site<'_>) {
    site.block(site.front, &Block::DIRT);
}

fn grass_front(site: &Site<'_>) {
    site.block(site.front, &Block::SHORT_GRASS);
}

fn floor_under_front(site: &Site<'_>) {
    site.block(site.front.down(), &Block::STONE);
}

fn rail_state(block: &Block, shape: RailShape) -> BlockStateId {
    let mut props = RailLikeProperties::default(block);
    props.shape = shape;
    props.to_state_id(block)
}

fn rail_front(site: &Site<'_>) {
    site.put(site.front, rail_state(&Block::RAIL, RailShape::NorthSouth));
}

fn rail_front_ascending(site: &Site<'_>) {
    site.put(
        site.front,
        rail_state(&Block::RAIL, RailShape::AscendingNorth),
    );
}

fn rail_below_front(site: &Site<'_>) {
    site.put(
        site.front.down(),
        rail_state(&Block::RAIL, RailShape::NorthSouth),
    );
}

fn rail_below_front_ascending(site: &Site<'_>) {
    site.put(
        site.front.down(),
        rail_state(&Block::RAIL, RailShape::AscendingEast),
    );
}

fn tnt_front(site: &Site<'_>) {
    site.block(site.front, &Block::TNT);
}

fn campfire_front(site: &Site<'_>) {
    site.block(site.front, &Block::CAMPFIRE);
}

fn copper_front(site: &Site<'_>) {
    site.block(site.front, &Block::COPPER_BLOCK);
}

fn beehive(site: &Site<'_>, honey_level: u8) {
    let mut props = BeeNestLikeProperties::default(&Block::BEEHIVE);
    props.honey_level = honey_level;
    site.put(site.front, props.to_state_id(&Block::BEEHIVE));
}

fn beehive_full(site: &Site<'_>) {
    beehive(site, 5);
}

fn beehive_low(site: &Site<'_>) {
    beehive(site, 2);
}

fn anchor(site: &Site<'_>, charges: u8) {
    let mut props = RespawnAnchorLikeProperties::default(&Block::RESPAWN_ANCHOR);
    props.charges = charges;
    site.put(site.front, props.to_state_id(&Block::RESPAWN_ANCHOR));
}

fn anchor_empty(site: &Site<'_>) {
    anchor(site, 0);
}

fn anchor_three(site: &Site<'_>) {
    anchor(site, 3);
}

fn anchor_full(site: &Site<'_>) {
    anchor(site, 4);
}

fn sheep_front(site: &Site<'_>) {
    site.mob(&EntityType::SHEEP);
}

fn sheared_sheep_front(site: &Site<'_>) {
    let sheep = site.mob(&EntityType::SHEEP);
    if let Some(sheep) = sheep.cast_any().downcast_ref::<SheepEntity>() {
        sheep.set_sheared(true);
    }
}

fn zombie_front(site: &Site<'_>) {
    site.mob(&EntityType::ZOMBIE);
}

fn dressed_zombie_front(site: &Site<'_>) {
    let zombie = site.mob(&EntityType::ZOMBIE);
    if let Some(living) = zombie.get_living_entity() {
        living
            .entity_equipment
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .put(&EquipmentSlot::CHEST, item(&Item::IRON_CHESTPLATE, 1));
    }
}

fn pig_front(site: &Site<'_>) {
    site.mob(&EntityType::PIG);
}

fn horse_front(site: &Site<'_>) {
    site.mob(&EntityType::HORSE);
}

fn llama_front(site: &Site<'_>) {
    site.mob(&EntityType::LLAMA);
}

fn snow_golem_pattern(site: &Site<'_>) {
    site.block(site.front.down(), &Block::SNOW_BLOCK);
    site.block(site.front.down().down(), &Block::SNOW_BLOCK);
}

fn wither_pattern(site: &Site<'_>) {
    let top_middle = site.front.down();
    let base = top_middle.down();
    for arm in [
        top_middle.offset(Vector3::new(0, 0, -1)),
        top_middle.offset(Vector3::new(0, 0, 1)),
    ] {
        site.block(arm, &Block::SOUL_SAND);
        site.block(arm.up(), &Block::WITHER_SKELETON_SKULL);
    }
    site.block(top_middle, &Block::SOUL_SAND);
    site.block(base, &Block::SOUL_SAND);
}

fn projectile_scenes() -> Vec<Scene> {
    vec![
        Scene::new("arrow x3", item(&Item::ARROW, 3)),
        Scene::new("arrow x1", item(&Item::ARROW, 1)),
        Scene::new("arrow x3 cancelled", item(&Item::ARROW, 3)).cancelled(),
        Scene::new("tipped arrow", item(&Item::TIPPED_ARROW, 2)),
        Scene::new("spectral arrow", item(&Item::SPECTRAL_ARROW, 2)),
        Scene::new("snowball", item(&Item::SNOWBALL, 16)),
        Scene::new("egg", item(&Item::EGG, 16)),
        Scene::new("splash potion", item(&Item::SPLASH_POTION, 1)),
        Scene::new("lingering potion", item(&Item::LINGERING_POTION, 2)),
        Scene::new("fire charge", item(&Item::FIRE_CHARGE, 3)),
        Scene::new("wind charge", item(&Item::WIND_CHARGE, 4)),
        Scene::new("firework rocket", item(&Item::FIREWORK_ROCKET, 2)),
        Scene::new("experience bottle", item(&Item::EXPERIENCE_BOTTLE, 5)),
        Scene::new("tnt", item(&Item::TNT, 4)),
        Scene::new("tnt rule off", item(&Item::TNT, 4)).tnt_off(),
    ]
}

fn spawner_scenes() -> Vec<Scene> {
    vec![
        Scene::new("armor stand", item(&Item::ARMOR_STAND, 2)),
        Scene::new("armor stand blocked", item(&Item::ARMOR_STAND, 2)).with(stone_front),
        Scene::new("boat on water", item(&Item::OAK_BOAT, 2)).with(water_front),
        Scene::new("boat over water", item(&Item::OAK_BOAT, 1)).with(water_below_front),
        Scene::new("boat on land", item(&Item::OAK_BOAT, 2)),
        Scene::new("boat crowded", item(&Item::OAK_BOAT, 2)).with(crowded_water),
        Scene::new("chest raft", item(&Item::BAMBOO_CHEST_RAFT, 1)).with(water_front),
        Scene::new("minecart on rail", item(&Item::MINECART, 2)).with(rail_front),
        Scene::new("minecart on slope", item(&Item::MINECART, 2)).with(rail_front_ascending),
        Scene::new("minecart rail below", item(&Item::MINECART, 2)).with(rail_below_front),
        Scene::new("minecart slope below", item(&Item::MINECART, 1))
            .with(rail_below_front_ascending),
        Scene::new("minecart no rail", item(&Item::MINECART, 2)),
        Scene::new("chest minecart", item(&Item::CHEST_MINECART, 2)).with(rail_front),
        Scene::new("zombie egg", item(&Item::ZOMBIE_SPAWN_EGG, 2)),
        Scene::new("pig egg last", item(&Item::PIG_SPAWN_EGG, 1)),
        Scene::new("egg blocked", item(&Item::ZOMBIE_SPAWN_EGG, 2)).with(stone_front),
    ]
}

fn fluid_scenes() -> Vec<Scene> {
    vec![
        Scene::new("bucket water", item(&Item::BUCKET, 1)).with(water_front),
        Scene::new("bucket lava", item(&Item::BUCKET, 1)).with(lava_front),
        Scene::new("bucket dry", item(&Item::BUCKET, 1)),
        Scene::new("buckets water room", item(&Item::BUCKET, 3)).with(water_front),
        Scene::new("buckets water full", item(&Item::BUCKET, 3))
            .with(water_front)
            .full(),
        Scene::new("water bucket air", item(&Item::WATER_BUCKET, 1)),
        Scene::new("water bucket stone", item(&Item::WATER_BUCKET, 1)).with(stone_front),
        Scene::new("lava bucket air", item(&Item::LAVA_BUCKET, 1)),
        Scene::new("snow bucket air", item(&Item::POWDER_SNOW_BUCKET, 1)),
        Scene::new("cod bucket air", item(&Item::COD_BUCKET, 1)),
        Scene::new("water bucket nether", item(&Item::WATER_BUCKET, 1)).nether(),
        Scene::new("lava bucket nether", item(&Item::LAVA_BUCKET, 1)).nether(),
        Scene::new("glass bottle water", item(&Item::GLASS_BOTTLE, 1)).with(water_front),
        Scene::new("glass bottles water room", item(&Item::GLASS_BOTTLE, 3)).with(water_front),
        Scene::new("glass bottles water full", item(&Item::GLASS_BOTTLE, 3))
            .with(water_front)
            .full(),
        Scene::new("glass bottle hive", item(&Item::GLASS_BOTTLE, 2)).with(beehive_full),
        Scene::new("glass bottle low hive", item(&Item::GLASS_BOTTLE, 2)).with(beehive_low),
        Scene::new("glass bottle dry", item(&Item::GLASS_BOTTLE, 2)),
        Scene::new("water bottle dirt", potion_of(&Potion::WATER)).with(dirt_front),
        Scene::new("water bottle stone", potion_of(&Potion::WATER)).with(stone_front),
        Scene::new("awkward potion dirt", potion_of(&Potion::AWKWARD)).with(dirt_front),
        Scene::new("plain potion dirt", item(&Item::POTION, 1)).with(dirt_front),
    ]
}

fn tool_scenes() -> Vec<Scene> {
    vec![
        Scene::new("flint on tnt", item(&Item::FLINT_AND_STEEL, 1)).with(tnt_front),
        Scene::new("flint on stone", item(&Item::FLINT_AND_STEEL, 1)).with(stone_front),
        Scene::new("flint on campfire", item(&Item::FLINT_AND_STEEL, 1)).with(campfire_front),
        Scene::new("flint on floor", item(&Item::FLINT_AND_STEEL, 1)).with(floor_under_front),
        Scene::new("honeycomb copper", item(&Item::HONEYCOMB, 3)).with(copper_front),
        Scene::new("honeycomb stone", item(&Item::HONEYCOMB, 3)).with(stone_front),
        Scene::new("shears hive", item(&Item::SHEARS, 1)).with(beehive_full),
        Scene::new("shears low hive", item(&Item::SHEARS, 1)).with(beehive_low),
        Scene::new("shears sheep", item(&Item::SHEARS, 1))
            .with(sheep_front)
            .loose(),
        Scene::new("shears sheared sheep", item(&Item::SHEARS, 1)).with(sheared_sheep_front),
        Scene::new("shears nothing", item(&Item::SHEARS, 1)),
        Scene::new("glowstone empty anchor", item(&Item::GLOWSTONE, 2)).with(anchor_empty),
        Scene::new("glowstone three anchor", item(&Item::GLOWSTONE, 1)).with(anchor_three),
        Scene::new("glowstone full anchor", item(&Item::GLOWSTONE, 2)).with(anchor_full),
        Scene::new("glowstone stone", item(&Item::GLOWSTONE, 2)).with(stone_front),
    ]
}

fn placed_scenes() -> Vec<Scene> {
    vec![
        Scene::new(
            "wither skull pattern",
            item(&Item::WITHER_SKELETON_SKULL, 2),
        )
        .with(wither_pattern),
        Scene::new("wither skull zombie", item(&Item::WITHER_SKELETON_SKULL, 2)).with(zombie_front),
        Scene::new(
            "wither skull nothing",
            item(&Item::WITHER_SKELETON_SKULL, 2),
        ),
        Scene::new("pumpkin golem", item(&Item::CARVED_PUMPKIN, 2)).with(snow_golem_pattern),
        Scene::new("pumpkin zombie", item(&Item::CARVED_PUMPKIN, 2)).with(zombie_front),
        Scene::new("pumpkin nothing", item(&Item::CARVED_PUMPKIN, 2)),
        Scene::new("shulker on floor", item(&Item::SHULKER_BOX, 2)).with(floor_under_front),
        Scene::new("shulker over air", item(&Item::SHULKER_BOX, 1)),
        Scene::new("shulker blocked", item(&Item::SHULKER_BOX, 2)).with(stone_front),
        Scene::new("shulker in grass", item(&Item::WHITE_SHULKER_BOX, 2)).with(grass_front),
        Scene::new("red shulker", item(&Item::RED_SHULKER_BOX, 2)).with(floor_under_front),
    ]
}

fn equipment_scenes() -> Vec<Scene> {
    vec![
        Scene::new("chestplate zombie", item(&Item::IRON_CHESTPLATE, 2)).with(zombie_front),
        Scene::new("chestplate worn", item(&Item::IRON_CHESTPLATE, 2)).with(dressed_zombie_front),
        Scene::new("chestplate nobody", item(&Item::IRON_CHESTPLATE, 2)),
        Scene::new("chestplate player", item(&Item::IRON_CHESTPLATE, 2)).player_in_front(),
        Scene::new("saddle pig", item(&Item::SADDLE, 1)).with(pig_front),
        Scene::new("saddle zombie", item(&Item::SADDLE, 1)).with(zombie_front),
        Scene::new("elytra zombie", item(&Item::ELYTRA, 1)).with(zombie_front),
        Scene::new("horse armor", item(&Item::IRON_HORSE_ARMOR, 1)).with(horse_front),
        Scene::new("llama carpet", item(&Item::WHITE_CARPET, 2)).with(llama_front),
        Scene::new("skeleton skull zombie", item(&Item::SKELETON_SKULL, 2)).with(zombie_front),
        Scene::new("player head zombie", item(&Item::PLAYER_HEAD, 1)).with(zombie_front),
    ]
}

fn drop_scenes() -> Vec<Scene> {
    vec![
        Scene::new("stone", item(&Item::STONE, 64)),
        Scene::new("diamond sword", item(&Item::DIAMOND_SWORD, 1)),
        Scene::new("bone meal", item(&Item::BONE_MEAL, 5)),
        Scene::new("brush", item(&Item::BRUSH, 1)),
        Scene::new("chest", item(&Item::CHEST, 3)),
        Scene::new("flint", item(&Item::FLINT, 3)),
        Scene::new("stone cancelled", item(&Item::STONE, 64)).cancelled(),
        Scene::new("tnt cancelled", item(&Item::TNT, 4)).cancelled(),
        Scene::new("ticked stone", item(&Item::STONE, 2)).ticked(),
        Scene::new("ticked arrow", item(&Item::ARROW, 1)).ticked(),
        Scene::new("ticked empty", ItemStack::EMPTY.clone()).ticked(),
    ]
}

fn scenes() -> Vec<Scene> {
    let mut all = projectile_scenes();
    all.extend(spawner_scenes());
    all.extend(fluid_scenes());
    all.extend(tool_scenes());
    all.extend(placed_scenes());
    all.extend(equipment_scenes());
    all.extend(drop_scenes());
    all
}

/// Remembers every `BlockDispenseEvent` and game event and cancels the dispense events at the
/// position it is told to.
#[derive(Default)]
struct Spy {
    log: Mutex<Vec<String>>,
    cancel_at: Mutex<Option<BlockPos>>,
}

impl Spy {
    fn note(&self, line: String) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }
}

impl EventHandler<BlockDispenseEvent> for Spy {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockDispenseEvent,
    ) -> BoxFuture<'a, ()> {
        let cancel = *self
            .cancel_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            == Some(event.block_pos);
        self.note(format!("dispense {} cancel {cancel}", event.item_name));
        if cancel {
            event.set_cancelled(true);
        }
        Box::pin(async {})
    }
}

impl EventHandler<GenericGameEvent> for Spy {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut GenericGameEvent,
    ) -> BoxFuture<'a, ()> {
        self.note(format!(
            "game {} ({:.2}, {:.2}, {:.2})",
            event.event_key, event.position.x, event.position.y, event.position.z
        ));
        Box::pin(async {})
    }
}

/// A world of the server with its listener and the chunks it has loaded.
struct Env {
    world: Arc<World>,
    listener: Arc<Player>,
    tap: PacketTap,
    loaded: BTreeSet<(i32, i32)>,
}

impl Env {
    fn new(world: &Arc<World>) -> Result<Self, Box<dyn std::error::Error>> {
        let (listener, tap) =
            test_support::java_player_with_tap(world, GameMode::Survival).ok_or("no packet tap")?;
        world.add_player(&listener)?;
        Ok(Self {
            world: world.clone(),
            listener,
            tap,
            loaded: BTreeSet::new(),
        })
    }

    fn load_around(&mut self, pos: BlockPos) {
        let chunk = pos.chunk_position();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let at = (chunk.x + dx, chunk.y + dz);
                if self.loaded.insert(at) {
                    self.world.level.loaded_chunks.insert(
                        Vector2::new(at.0, at.1),
                        pumpkin_world::chunk::ChunkData::empty_sync(at.0, at.1),
                    );
                }
            }
        }
    }

    fn put_listener(&self, at: Vector3<f64>, pos: BlockPos) {
        self.listener.get_entity().set_pos(at);
        self.listener.watched_section.store(Cylindrical::new(
            pos.chunk_position(),
            NonZero::<u8>::MIN.saturating_add(2),
        ));
    }
}

fn set_tnt(world: &World, on: bool) {
    world.level_info.rcu(|info| {
        let mut next = (**info).clone();
        next.game_rules.tnt_explodes = on;
        Arc::new(next)
    });
}

fn dispenser_pos(index: usize) -> BlockPos {
    let column = (index % COLUMNS) as i32;
    let row = (index / COLUMNS) as i32;
    BlockPos::new(column * SPACING + 4, 64, row * SPACING + 4)
}

fn dispenser_at(
    world: &Arc<World>,
    pos: BlockPos,
    facing: Facing,
) -> Result<Arc<dyn crate::block::entities::BlockEntity>, &'static str> {
    let mut props = DispenserLikeProperties::default(&Block::DISPENSER);
    props.facing = facing;
    world.set_block_state(
        &pos,
        props.to_state_id(&Block::DISPENSER),
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
    world.add_block_entity(Arc::new(DispenserBlockEntity::new(pos)));
    world
        .get_block_entity(&pos)
        .ok_or("the dispenser has no block entity")
}

/// The entities spawned by the scene, then the ones that stood around the dispenser.
fn surroundings(
    world: &World,
    pos: BlockPos,
    scene: &Scene,
    known: &HashSet<i32>,
    standing: &[Arc<dyn EntityBase>],
) -> String {
    let mut spawned: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter(|entity| !known.contains(&entity.get_entity().entity_id))
        .map(|entity| entity_text(entity, scene.loose))
        .collect();
    if scene.loose {
        spawned.dedup();
    }
    let mobs: Vec<String> = standing
        .iter()
        .filter(|entity| {
            let at = entity.get_entity().pos.load();
            (at.x - f64::from(pos.0.x)).abs() < 4.0 && (at.z - f64::from(pos.0.z)).abs() < 4.0
        })
        .map(|entity| equipment_text(entity.as_ref()))
        .collect();
    format!("{}] near [{}]", spawned.join("; "), mobs.join("; "))
}

fn run_scene(
    env: &mut Env,
    spy: &Spy,
    index: usize,
    scene: &Scene,
    facing: Facing,
) -> Result<String, Box<dyn std::error::Error>> {
    let pos = dispenser_pos(index);
    env.load_around(pos);
    let world = env.world.clone();
    let block_entity = dispenser_at(&world, pos, facing)?;
    let dispenser = block_entity
        .as_any()
        .downcast_ref::<DispenserBlockEntity>()
        .ok_or("not a dispenser")?;
    let ctx = DispenseContext {
        world: &world,
        position: &pos,
        facing,
    };
    let site = Site {
        world: &world,
        pos,
        front: DispenserBlock::target_position(&ctx),
    };
    let above = Vector3::new(
        f64::from(pos.0.x) + 0.5,
        f64::from(pos.0.y) + 5.0,
        f64::from(pos.0.z) + 0.5,
    );
    env.put_listener(above, pos);
    if scene.player_in_front {
        env.put_listener(site.front.to_centered_f64(), pos);
    }
    (scene.setup)(&site);
    if scene.full {
        let mut items = dispenser
            .items
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        for slot in items.iter_mut().skip(1) {
            *slot = item(&Item::COBBLESTONE, 1);
        }
    }
    set_tnt(&world, !scene.tnt_off);
    *spy.cancel_at.lock().unwrap_or_else(PoisonError::into_inner) = scene.cancelled.then_some(pos);
    let before = region(&world, pos);
    let standing: Vec<Arc<dyn EntityBase>> = world.entities.load().iter().cloned().collect();
    let known: HashSet<i32> = standing.iter().map(|e| e.get_entity().entity_id).collect();
    drop(env.tap.drain());
    spy.log
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();

    let mut stack = scene.item.clone();
    dispenser
        .items
        .write()
        .unwrap_or_else(PoisonError::into_inner)[0] = stack.clone();
    if scene.ticked {
        DispenserBlock.on_scheduled_tick(OnScheduledTickArgs {
            world: &world,
            block: &Block::DISPENSER,
            position: &pos,
        });
        stack = ItemStack::EMPTY.clone();
    } else {
        DispenserBlock::dispense(&ctx, dispenser, &mut stack);
        dispenser
            .items
            .write()
            .unwrap_or_else(PoisonError::into_inner)[0] = stack.clone();
    }
    set_tnt(&world, true);

    let mut text = format!(
        "{} {facing:?}: left [{}] slots [{}] heard [{}] log {:?} spawned [",
        scene.name,
        stack_text(&stack),
        slots_text(dispenser),
        heard(&mut env.tap),
        spy.log.lock().unwrap_or_else(PoisonError::into_inner),
    );
    text.push_str(&surroundings(&world, pos, scene, &known, &standing));
    if scene.player_in_front {
        let _ = write!(text, " player [{}]", equipment_text(env.listener.as_ref()));
    }
    let _ = write!(text, " blocks{}", changes_text(&world, pos, &before));
    Ok(text)
}

async fn run(server: &Arc<Server>) -> Result<String, Box<dyn std::error::Error>> {
    let worlds = server.worlds.load();
    let overworld = worlds[0].clone();
    let nether = worlds
        .iter()
        .find(|world| world.dimension.fast_lava)
        .ok_or("no fast-lava dimension")?
        .clone();
    let spy = Arc::new(Spy::default());
    server.plugin_manager.register::<BlockDispenseEvent, _>(
        spy.clone(),
        EventPriority::Normal,
        true,
    );
    server
        .plugin_manager
        .register::<GenericGameEvent, _>(spy.clone(), EventPriority::Normal, true);
    let mut envs = [Env::new(&overworld)?, Env::new(&nether)?];
    let mut out = String::new();
    for (index, scene) in scenes().iter().enumerate() {
        for (number, facing) in FACINGS.into_iter().enumerate() {
            let env = &mut envs[usize::from(scene.nether)];
            out.push_str(&run_scene(
                env,
                &spy,
                index * FACINGS.len() + number,
                scene,
                facing,
            )?);
            out.push('\n');
        }
    }
    for env in &envs {
        env.world.remove_player(&env.listener, false).await;
    }
    Ok(out)
}

#[tokio::test(flavor = "multi_thread")]
async fn dispenser_dispense() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let out = run(&server).await?;
    server.stop();
    script_golden::bless_or_compare("dispenser_dispense.txt", &out)
}

/// Vanilla's boat behaviour puts the boat on the water in front of the dispenser. Here the
/// room check box (1.375 wide, centred on the front block) overlaps the dispenser itself, so the
/// boat is dropped as an item instead: the scene is red until that is fixed (card on board #18).
#[tokio::test(flavor = "multi_thread")]
async fn boat_on_water_in_front_spawns_a_boat() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut env = Env::new(&world)?;
    let spy = Spy::default();
    let scene = Scene::new("boat on water", item(&Item::OAK_BOAT, 2)).with(water_front);
    let line = run_scene(&mut env, &spy, 0, &scene, Facing::North)?;
    world.remove_player(&env.listener, false).await;
    server.stop();
    assert!(line.contains("spawned [oak_boat"), "{line}");
    Ok(())
}
