//! A record of what `VillagerEntity::mob_write_nbt` writes, made before the function was split
//! (`tests/golden/villager_nbt.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene puts a fresh villager on a real server's world, sets the fields the function reads
//! (villager data, food, xp, restock and gossip times, job site and home, offers, inventory,
//! gossips) and calls `mob_write_nbt` on an empty compound. The line holds the written compound
//! as text with every compound's keys sorted. The profession, type and level ids are raw numbers
//! (some beyond the registries), so a swapped or dropped field shows. Gossips keep the order the
//! villager's (deterministic) hash map gives.

use std::borrow::Cow;
use std::fmt::Write;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::data_component_impl::DamageImpl;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::MerchantOffer;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::entity::mob::Mob;
use crate::entity::passive::villager::{GossipType, VillagerData, VillagerEntity};

/// What the villager's inventory holds.
#[derive(Clone, Copy)]
enum Inventory {
    /// The eight empty slots a new villager has.
    Fresh,
    /// No slots at all.
    Cleared,
    /// Three stacks, one with a damage component.
    Items,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    /// Raw type, profession and level ids.
    data: (i32, i32, i32),
    food: i32,
    xp: i32,
    last_restock: i64,
    restocks_today: i32,
    last_decay: i64,
    last_share: i64,
    last_worked: i64,
    /// Job site position and its pending flag.
    job_site: Option<((i32, i32, i32), bool)>,
    /// A pending flag with no job site must not be written.
    stray_pending: bool,
    home: Option<(i32, i32, i32)>,
    offers: fn() -> Vec<MerchantOffer>,
    inventory: Inventory,
    gossips: fn() -> Vec<(Uuid, GossipType, i32)>,
}

fn no_offers() -> Vec<MerchantOffer> {
    Vec::new()
}

fn no_gossips() -> Vec<(Uuid, GossipType, i32)> {
    Vec::new()
}

const BASE: Scene = Scene {
    name: "",
    data: (0, 0, 1),
    food: 0,
    xp: 0,
    last_restock: 0,
    restocks_today: 0,
    last_decay: 0,
    last_share: 0,
    last_worked: 0,
    job_site: None,
    stray_pending: false,
    home: None,
    offers: no_offers,
    inventory: Inventory::Fresh,
    gossips: no_gossips,
};

const fn scene(name: &'static str) -> Scene {
    Scene { name, ..BASE }
}

fn stack(count: u8, item: &'static Item) -> ItemStack {
    ItemStack::new(count, item)
}

fn serial(stack: ItemStack) -> ItemStackSerializer<'static> {
    ItemStackSerializer(Cow::Owned(stack))
}

/// One offer: both costs, the output and every number the function writes.
#[allow(clippy::too_many_arguments)]
fn offer(
    cost_a: ItemStack,
    cost_b: Option<ItemStack>,
    output: ItemStack,
    (uses, max_uses, xp): (i32, i32, i32),
    (special_price, demand): (i32, i32),
    price_multiplier: f32,
    reward_exp: bool,
) -> MerchantOffer {
    MerchantOffer {
        base_cost_a: serial(cost_a),
        output: serial(output),
        cost_b: cost_b.map(serial),
        reward_exp,
        uses,
        max_uses,
        xp,
        special_price,
        price_multiplier,
        demand,
    }
}

fn one_offer() -> Vec<MerchantOffer> {
    vec![offer(
        stack(24, &Item::PAPER),
        None,
        stack(1, &Item::EMERALD),
        (0, 16, 2),
        (0, 0),
        0.05,
        true,
    )]
}

fn used_offer() -> Vec<MerchantOffer> {
    vec![offer(
        stack(3, &Item::EMERALD),
        Some(stack(1, &Item::BOOK)),
        stack(1, &Item::BOOKSHELF),
        (5, 12, 7),
        (-3, 9),
        0.2,
        false,
    )]
}

fn three_offers() -> Vec<MerchantOffer> {
    let mut damaged = stack(1, &Item::IRON_SWORD);
    damaged.set_data_component(DamageImpl { damage: 11 });
    let mut offers = used_offer();
    offers.extend(one_offer());
    offers.push(offer(
        stack(9, &Item::EMERALD),
        Some(stack(2, &Item::DIAMOND)),
        damaged,
        (16, 16, 30),
        (4, -2),
        2.5,
        true,
    ));
    offers
}

fn uuid(high: u64, low: u64) -> Uuid {
    Uuid::from_u64_pair(high, low)
}

fn one_gossip() -> Vec<(Uuid, GossipType, i32)> {
    vec![(
        uuid(0x1234_5678_9abc_def0, 0xfedc_ba98_7654_3210),
        GossipType::MajorPositive,
        25,
    )]
}

fn many_gossips() -> Vec<(Uuid, GossipType, i32)> {
    let first = uuid(0x8000_0001_7fff_ffff, 0x0000_0002_8000_0003);
    let second = uuid(1, u64::MAX);
    vec![
        (first, GossipType::MajorNegative, -20),
        (first, GossipType::Trading, 6),
        (first, GossipType::MinorNegative, 30),
        (second, GossipType::MinorPositive, 1),
        (second, GossipType::MajorPositive, 100),
    ]
}

fn data_scenes() -> Vec<Scene> {
    vec![
        scene("blank"),
        Scene {
            data: (2, 5, 3),
            ..scene("data: swamp librarian level 3")
        },
        Scene {
            data: (6, 14, 5),
            ..scene("data: last ids")
        },
        Scene {
            data: (123, -7, 0),
            ..scene("data: odd numbers")
        },
        Scene {
            food: 12,
            xp: 450,
            last_restock: 123_456_789_012,
            restocks_today: 2,
            last_decay: 98_765_432_109,
            ..scene("counters")
        },
        Scene {
            food: -1,
            xp: -300,
            last_restock: -5,
            restocks_today: -2,
            last_decay: -9,
            ..scene("negative counters")
        },
    ]
}

fn time_scenes() -> Vec<Scene> {
    vec![
        Scene {
            last_share: 1,
            ..scene("share: one")
        },
        Scene {
            last_share: 77_000,
            ..scene("share: set")
        },
        Scene {
            last_share: -4,
            ..scene("share: negative")
        },
        Scene {
            last_worked: 1,
            ..scene("worked: one")
        },
        Scene {
            last_worked: 31_000,
            ..scene("worked: set")
        },
        Scene {
            last_worked: -4,
            ..scene("worked: negative")
        },
        Scene {
            last_share: 5,
            last_worked: 6,
            ..scene("share and worked")
        },
    ]
}

fn place_scenes() -> Vec<Scene> {
    vec![
        Scene {
            job_site: Some(((10, 64, -20), false)),
            ..scene("job site: not pending")
        },
        Scene {
            job_site: Some(((-300, -5, 4000), true)),
            ..scene("job site: pending, negative coordinates")
        },
        Scene {
            stray_pending: true,
            ..scene("pending without a job site")
        },
        Scene {
            home: Some((1, 2, 3)),
            ..scene("home only")
        },
        Scene {
            home: Some((-100, 70, 250)),
            job_site: Some(((7, 8, 9), true)),
            ..scene("home and job site")
        },
    ]
}

fn list_scenes() -> Vec<Scene> {
    vec![
        Scene {
            offers: one_offer,
            ..scene("offers: one plain")
        },
        Scene {
            offers: used_offer,
            ..scene("offers: second cost, uses, price changes")
        },
        Scene {
            offers: three_offers,
            ..scene("offers: three")
        },
        Scene {
            inventory: Inventory::Cleared,
            ..scene("inventory: none")
        },
        Scene {
            inventory: Inventory::Items,
            ..scene("inventory: items")
        },
        Scene {
            gossips: one_gossip,
            ..scene("gossips: one")
        },
        Scene {
            gossips: many_gossips,
            ..scene("gossips: many")
        },
    ]
}

fn everything_scene() -> Scene {
    Scene {
        name: "everything",
        data: (4, 9, 2),
        food: 3,
        xp: 55,
        last_restock: 11,
        restocks_today: 1,
        last_decay: 12,
        last_share: 13,
        last_worked: 14,
        job_site: Some(((1, 2, 3), true)),
        stray_pending: false,
        home: Some((4, 5, 6)),
        offers: three_offers,
        inventory: Inventory::Items,
        gossips: many_gossips,
    }
}

fn scenes() -> Vec<Scene> {
    let mut all = data_scenes();
    all.extend(time_scenes());
    all.extend(place_scenes());
    all.extend(list_scenes());
    all.push(everything_scene());
    all
}

fn inventory_items(kind: Inventory) -> Vec<ItemStack> {
    match kind {
        Inventory::Fresh => (0..8).map(|_| ItemStack::EMPTY.clone()).collect(),
        Inventory::Cleared => Vec::new(),
        Inventory::Items => {
            let mut sword = stack(1, &Item::DIAMOND_SWORD);
            sword.set_data_component(DamageImpl { damage: 3 });
            vec![
                stack(64, &Item::WHEAT),
                ItemStack::EMPTY.clone(),
                sword,
                stack(5, &Item::CARROT),
            ]
        }
    }
}

fn apply(villager: &VillagerEntity, scene: &Scene) {
    let (kind, profession, level) = scene.data;
    *villager.villager_data.lock().unwrap() = VillagerData {
        r#type: VarInt(kind),
        profession: VarInt(profession),
        level: VarInt(level),
    };
    villager.food_level.store(scene.food, Relaxed);
    villager.xp.store(scene.xp, Relaxed);
    villager
        .last_restock_time
        .store(scene.last_restock, Relaxed);
    villager.restocks_today.store(scene.restocks_today, Relaxed);
    villager
        .last_gossip_decay_time
        .store(scene.last_decay, Relaxed);
    villager
        .last_gossip_share_time
        .store(scene.last_share, Relaxed);
    villager
        .last_worked_at_poi
        .store(scene.last_worked, Relaxed);
    if let Some(((x, y, z), pending)) = scene.job_site {
        *villager.job_site.lock().unwrap() = Some(BlockPos::new(x, y, z));
        villager.job_site_pending.store(pending, Relaxed);
    }
    villager
        .job_site_pending
        .fetch_or(scene.stray_pending, Relaxed);
    if let Some((x, y, z)) = scene.home {
        *villager.home_pos.lock().unwrap() = Some(BlockPos::new(x, y, z));
    }
    *villager.offers.lock().unwrap() = (scene.offers)();
    *villager.inventory.lock().unwrap() = inventory_items(scene.inventory);
    let mut gossips = villager.gossips.lock().unwrap();
    for (target, kind, value) in (scene.gossips)() {
        gossips.entry(target).or_default().insert(kind, value);
    }
}

/// NBT as text with the compound keys sorted.
fn nbt_text(tag: &NbtTag, out: &mut String) {
    match tag {
        NbtTag::Compound(compound) => compound_text(compound, out),
        NbtTag::List(items) => {
            out.push('[');
            for item in items {
                nbt_text(item, out);
                out.push(',');
            }
            out.push(']');
        }
        other => {
            let _ = write!(out, "{other:?}");
        }
    }
}

fn compound_text(compound: &NbtCompound, out: &mut String) {
    let mut keys: Vec<&str> = compound.child_tags.keys().map(AsRef::as_ref).collect();
    keys.sort_unstable();
    out.push('{');
    for key in keys {
        let _ = write!(out, "{key}:");
        if let Some(tag) = compound.child_tags.get(key) {
            nbt_text(tag, out);
        }
        out.push(',');
    }
    out.push('}');
}

#[tokio::test(flavor = "multi_thread")]
async fn villager_nbt_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut out = String::new();
    for (index, scene) in scenes().iter().enumerate() {
        let entity = Entity::from_uuid_with_id(
            index as i32 + 5000,
            Uuid::from_u128(0x7669_6c00_0000 + index as u128),
            world.clone(),
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::VILLAGER,
        );
        let villager = VillagerEntity::new(entity);
        apply(&villager, scene);
        let mut written = NbtCompound::new();
        villager.mob_write_nbt(&mut written);
        let _ = write!(out, "{}: ", scene.name);
        compound_text(&written, &mut out);
        out.push('\n');
    }
    server.stop();
    script_golden::bless_or_compare("villager_nbt.txt", &out)
}
