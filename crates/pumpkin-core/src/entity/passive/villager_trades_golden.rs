//! A record of what `VillagerEntity::add_trades` does, made before the function was split
//! (`tests/golden/villager_trades.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The function draws its trades with an unseeded generator, so a run cannot be replayed. A
//! scene therefore calls it many times for one profession and level and records what can come
//! out: the numbers of offers added per call, and every distinct offer (items, counts, uses,
//! xp, price multiplier), the villager types that give the same result grouped on one header.
//! Random parts (the enchantments, a dye's colour, a map's id) print as a mask.
//!
//! Two sections: the shipped trade sets of every profession at levels 0 to 6 (the server's own
//! data), then synthetic sets from a datapack written into the test world (every modifier, the
//! type filter, defaults, fractional counts, an amount beyond the trades, unresolvable trades,
//! the clamp and the zero count of an enchanted cost). A synthetic set takes the place of the
//! profession's shipped one, so it runs after the shipped section.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{
    DyedColorImpl, EnchantmentsImpl, ItemNameImpl, MapIdImpl, PotionContentsImpl,
    StoredEnchantmentsImpl, SuspiciousStewEffectsImpl,
};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::java::client::play::MerchantOffer;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::entity::passive::villager::{
    VillagerData, VillagerEntity, VillagerProfession, VillagerType,
};
use crate::world::World;

const TYPES: [VillagerType; 7] = [
    VillagerType::Desert,
    VillagerType::Jungle,
    VillagerType::Plains,
    VillagerType::Savanna,
    VillagerType::Snow,
    VillagerType::Swamp,
    VillagerType::Taiga,
];

/// Calls per profession, level and type for the shipped sets.
const SHIPPED_RUNS: usize = 150;

/// What one scene saw: the offer counts of its calls and every distinct offer.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Seen {
    counts: BTreeSet<usize>,
    offers: BTreeSet<String>,
}

/// One synthetic set: its trades (names in [`TRADES`]), the amount written in the file, the
/// villager types it runs for, how many calls, and whether the costs are printed as they are
/// (a masked cost count hides what the enchant arithmetic does).
struct Synth {
    name: &'static str,
    amount: &'static str,
    trades: &'static [&'static str],
    types: &'static [VillagerType],
    runs: usize,
    exact: bool,
}

const PLAINS: &[VillagerType] = &[VillagerType::Plains];

const fn synth(
    name: &'static str,
    amount: &'static str,
    trades: &'static [&'static str],
    runs: usize,
) -> Synth {
    Synth {
        name,
        amount,
        trades,
        types: PLAINS,
        runs,
        exact: true,
    }
}

const POTION: &str = r#","given_item_modifier":[{"type":"minecraft:set_potion","id":"#;

/// Trade files of the synthetic pack: name and JSON (the modifier and merchant predicate keys
/// are the datapack's own).
fn trades() -> Vec<(&'static str, String)> {
    let enchant = |item: &str, count: u32, min: i32, max: i32| {
        format!(
            r#"{{"wants":{{"id":"minecraft:emerald","count":{count}}},"gives":{{"id":"minecraft:{item}"}},"given_item_modifier":[{{"type":"minecraft:enchant_with_levels","levels":{{"min":{min},"max":{max}}}}}]}}"#
        )
    };
    let plain = |wants: &str, gives: &str, extra: &str| {
        format!(
            r#"{{"wants":{{"id":"minecraft:{wants}"}},"gives":{{"id":"minecraft:{gives}"}}{extra}}}"#
        )
    };
    let kind = |gives: &str, variants: &str| {
        plain(
            "emerald",
            gives,
            &format!(
                r#","merchant_predicate":{{"predicate":{{"minecraft:predicates":{{"minecraft:villager/variant":{variants}}}}}}}"#
            ),
        )
    };
    let map = |gives: &str, destination: &str| {
        plain(
            "emerald",
            gives,
            &format!(
                r#","given_item_modifier":[{{"type":"minecraft:exploration_map","destination":"{destination}"}}]"#
            ),
        )
    };
    vec![
        (
            "plain",
            r#"{"wants":{"id":"minecraft:emerald","count":3},"gives":{"id":"minecraft:bread","count":5},"additional_wants":{"id":"minecraft:paper","count":2},"max_uses":7,"xp":4,"reputation_discount":0.25}"#.to_owned(),
        ),
        ("defaults", plain("emerald", "apple", "")),
        (
            "fractional",
            r#"{"wants":{"id":"minecraft:emerald","count":2.7},"gives":{"id":"minecraft:bread","count":4.9},"max_uses":9.5,"xp":3.5}"#.to_owned(),
        ),
        ("desert_only", kind("cactus", r#""minecraft:desert""#)),
        (
            "swamp_jungle",
            kind("vine", r#"["minecraft:swamp","minecraft:jungle"]"#),
        ),
        ("anywhere", plain("emerald", "stick", "")),
        ("bad_item", plain("not_an_item", "stick", "")),
        (
            "enchant_random",
            plain("emerald", "book", r#","additional_wants":{"id":"minecraft:paper"},"given_item_modifier":[{"type":"minecraft:enchant_randomly"}]"#),
        ),
        ("enchant_fixed", enchant("diamond_sword", 5, 10, 10)),
        ("enchant_clamp", enchant("diamond_pickaxe", 60, 10, 10)),
        ("enchant_zero", enchant("iron_axe", 0, 0, 0)),
        ("enchant_none", enchant("stick", 5, 5, 5)),
        ("map_a", map("map", "#minecraft:on_woodland_explorer_maps")),
        ("map_b", map("filled_map", "minecraft:on_ocean_monument_maps")),
        ("map_bad", map("map", "minecraft:nowhere")),
        (
            "dyes",
            plain("emerald", "leather_chestplate", r#","given_item_modifier":[{"type":"minecraft:set_random_dyes"}]"#),
        ),
        (
            "random_potion",
            plain("emerald", "potion", r#","given_item_modifier":[{"type":"minecraft:set_random_potion"}]"#),
        ),
        (
            "stew",
            plain("emerald", "suspicious_stew", r#","given_item_modifier":[{"type":"minecraft:set_stew_effect"}]"#),
        ),
        (
            "potion_known",
            plain("emerald", "potion", &format!(r#"{POTION}"minecraft:healing"}}]"#)),
        ),
        (
            "potion_unknown",
            plain("emerald", "potion", &format!(r#"{POTION}"minecraft:nonsense"}}]"#)),
        ),
    ]
}

fn synthetic() -> Vec<Synth> {
    let all: &'static [VillagerType] = &TYPES;
    let mut out = vec![
        synth("plain", "1", &["plain"], 3),
        synth("defaults", "1", &["defaults"], 3),
        synth("fractional", "1", &["fractional"], 3),
        synth("amount beyond the trades", "5", &["plain", "defaults"], 5),
        synth("amount zero", "0", &["plain"], 5),
        synth(
            "amount 1.5",
            "1.5",
            &["plain", "defaults", "fractional"],
            60,
        ),
        synth(
            "amount 1.4",
            "1.4",
            &["plain", "defaults", "fractional"],
            60,
        ),
        synth("unresolvable trade", "2", &["bad_item", "plain"], 5),
        synth("no trades", "2", &[], 5),
        Synth {
            exact: false,
            ..synth("enchanted book", "1", &["enchant_random"], 60)
        },
        synth("enchant levels", "1", &["enchant_fixed"], 60),
        synth("enchant levels clamp", "1", &["enchant_clamp"], 60),
        synth("enchant levels zero cost", "1", &["enchant_zero"], 20),
        synth("enchant nothing fits", "1", &["enchant_none"], 20),
        synth("map", "1", &["map_a"], 1),
        synth("map, older item", "1", &["map_b"], 1),
        synth("map, unknown destination", "1", &["map_bad"], 3),
        synth("random dyes", "1", &["dyes"], 60),
        Synth {
            exact: false,
            ..synth("random potion", "1", &["random_potion"], 60)
        },
        synth("random potion, ids", "1", &["random_potion"], 400),
        synth("suspicious stew", "1", &["stew"], 200),
        synth("potion", "1", &["potion_known"], 3),
        synth("unknown potion", "1", &["potion_unknown"], 3),
    ];
    out.push(Synth {
        types: all,
        ..synth(
            "type filter",
            "3",
            &["desert_only", "swamp_jungle", "anywhere", "plain"],
            150,
        )
    });
    out.push(Synth {
        types: all,
        ..synth(
            "type filter, amount 1",
            "1",
            &["desert_only", "swamp_jungle"],
            150,
        )
    });
    out
}

fn text_of(stack: &ItemStack, exact: bool) -> String {
    let mut text = format!("{}x{}", stack.item.registry_key, stack.item_count);
    for (component, _) in &stack.patch {
        let _ = write!(text, " [{}]", component_text(stack, *component, exact));
    }
    text
}

fn component_text(stack: &ItemStack, component: DataComponent, exact: bool) -> String {
    match component {
        DataComponent::PotionContents => stack
            .get_data_component::<PotionContentsImpl>()
            .map_or_else(
                || "potion ?".to_owned(),
                |p| {
                    if exact {
                        format!("potion {:?}", p.potion_id)
                    } else {
                        "potion".to_owned()
                    }
                },
            ),
        DataComponent::SuspiciousStewEffects => stack
            .get_data_component::<SuspiciousStewEffectsImpl>()
            .map_or_else(
                || "stew ?".to_owned(),
                |s| {
                    let effects: Vec<String> = s
                        .effects
                        .iter()
                        .map(|e| format!("{}:{}", e.effect, e.duration))
                        .collect();
                    format!("stew {}", effects.join(","))
                },
            ),
        DataComponent::Enchantments => stack
            .get_data_component::<EnchantmentsImpl>()
            .map_or_else(|| "enchantments ?".to_owned(), |_| "enchanted".to_owned()),
        DataComponent::StoredEnchantments => stack
            .get_data_component::<StoredEnchantmentsImpl>()
            .map_or_else(
                || "stored ?".to_owned(),
                |_| "stored enchantment".to_owned(),
            ),
        DataComponent::DyedColor => stack
            .get_data_component::<DyedColorImpl>()
            .map_or_else(|| "dye ?".to_owned(), |_| "dyed".to_owned()),
        DataComponent::MapId => stack
            .get_data_component::<MapIdImpl>()
            .map_or_else(|| "map ?".to_owned(), |_| "map id".to_owned()),
        DataComponent::ItemName => stack
            .get_data_component::<ItemNameImpl>()
            .map_or_else(|| "name ?".to_owned(), |n| format!("name {}", n.name)),
        other => format!("{other:?}"),
    }
}

fn offer_text(offer: &MerchantOffer, exact: bool) -> String {
    let output = &*offer.output.0;
    let enchanted = output.has_data_component(DataComponent::Enchantments)
        || output.has_data_component(DataComponent::StoredEnchantments);
    let mut cost_a = text_of(&offer.base_cost_a.0, exact);
    if enchanted && !exact {
        // The cost of an enchanted offer is drawn too: hide its count.
        cost_a = format!(
            "{}x? {}",
            offer.base_cost_a.0.item.registry_key,
            cost_a.split_once(' ').map_or("", |(_, rest)| rest)
        )
        .trim_end()
        .to_owned();
    }
    let cost_b = offer
        .cost_b
        .as_ref()
        .map_or_else(|| "-".to_owned(), |b| text_of(&b.0, exact));
    format!(
        "{cost_a} + {cost_b} -> {} | uses {} max {} xp {} special {} multiplier {} demand {} reward {}",
        text_of(output, exact),
        offer.uses,
        offer.max_uses,
        offer.xp,
        offer.special_price,
        offer.price_multiplier,
        offer.demand,
        offer.reward_exp,
    )
}

fn clear_offers(villager: &VillagerEntity) {
    villager
        .offers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

fn observe(
    villager: &VillagerEntity,
    profession: VillagerProfession,
    level: i32,
    runs: usize,
    exact: bool,
) -> Seen {
    let mut seen = Seen {
        counts: BTreeSet::new(),
        offers: BTreeSet::new(),
    };
    for _ in 0..runs {
        clear_offers(villager);
        villager.add_trades(profession, level);
        let offers = villager
            .offers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.counts.insert(offers.len());
        for offer in offers.iter() {
            seen.offers.insert(offer_text(offer, exact));
        }
    }
    seen
}

fn set_type(villager: &VillagerEntity, kind: VillagerType, profession: VillagerProfession) {
    *villager
        .villager_data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        VillagerData::new(kind, profession, 1);
}

/// Runs one profession and level for the given types and prints the groups of types that saw
/// the same.
fn scene(
    villager: &VillagerEntity,
    title: &str,
    profession: VillagerProfession,
    level: i32,
    types: &[VillagerType],
    runs: usize,
    exact: bool,
) -> String {
    let mut groups: BTreeMap<Seen, Vec<VillagerType>> = BTreeMap::new();
    for kind in types {
        set_type(villager, *kind, profession);
        groups
            .entry(observe(villager, profession, level, runs, exact))
            .or_default()
            .push(*kind);
    }
    let mut out = String::new();
    for (seen, kinds) in groups {
        let _ = writeln!(out, "{title} {kinds:?}: offers per call {:?}", seen.counts);
        for offer in seen.offers {
            let _ = writeln!(out, "    {offer}");
        }
    }
    out
}

fn shipped(villager: &VillagerEntity) -> String {
    let mut out = String::from("== shipped trade sets ==\n");
    for id in 0..=14 {
        let Some(profession) = VillagerProfession::from_i32(id) else {
            continue;
        };
        for level in 0..=6 {
            // Locating a map takes a second; the synthetic map scenes cover those trades.
            if profession == VillagerProfession::Cartographer && (2..=3).contains(&level) {
                continue;
            }
            let title = format!("{profession:?} level {level}");
            out.push_str(&scene(
                villager,
                &title,
                profession,
                level,
                &TYPES,
                SHIPPED_RUNS,
                false,
            ));
        }
    }
    out
}

fn write_pack(root: &std::path::Path, scenes: &[Synth]) -> TestResult {
    let data = root.join("data/minecraft");
    std::fs::create_dir_all(data.join("villager_trade"))?;
    std::fs::write(
        root.join("pack.mcmeta"),
        r#"{"pack":{"pack_format":99,"description":"villager trades golden"}}"#,
    )?;
    for (name, json) in trades() {
        std::fs::write(data.join(format!("villager_trade/gold_{name}.json")), json)?;
    }
    for (index, scene) in scenes.iter().enumerate() {
        let (profession, level) = slot(index);
        let dir = data.join(format!("trade_set/{}", profession.to_name()));
        std::fs::create_dir_all(&dir)?;
        let trades: Vec<String> = scene
            .trades
            .iter()
            .map(|name| format!("\"minecraft:gold_{name}\""))
            .collect();
        std::fs::write(
            dir.join(format!("level_{level}.json")),
            format!(
                r#"{{"amount":{},"trades":[{}]}}"#,
                scene.amount,
                trades.join(",")
            ),
        )?;
    }
    Ok(())
}

/// The profession and level a synthetic scene's set is written for.
fn slot(index: usize) -> (VillagerProfession, i32) {
    let profession =
        VillagerProfession::from_i32(1 + (index / 5) as i32).unwrap_or(VillagerProfession::None);
    (profession, 1 + (index % 5) as i32)
}

fn synthetic_text(villager: &VillagerEntity, scenes: &[Synth]) -> String {
    let mut out = String::from("== synthetic datapack sets ==\n");
    for (index, synth) in scenes.iter().enumerate() {
        let (profession, level) = slot(index);
        let title = format!("{} (amount {})", synth.name, synth.amount);
        out.push_str(&scene(
            villager,
            &title,
            profession,
            level,
            synth.types,
            synth.runs,
            synth.exact,
        ));
    }
    out
}

fn spawn(world: &Arc<World>) -> Result<Arc<VillagerEntity>, String> {
    let at = Vector3::new(300.5, 64.0, -200.5);
    let entity = Entity::from_uuid_with_id(
        4_000,
        Uuid::from_u128(0x7669_6c00_7472_0000),
        world.clone(),
        at,
        &EntityType::VILLAGER,
    );
    let villager = VillagerEntity::new(entity);
    if world.spawn_entity(villager.clone()) {
        Ok(villager)
    } else {
        Err("villager not spawned".to_owned())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn villager_add_trades_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let chunk = pumpkin_util::math::position::BlockPos::new(300, 64, -200).chunk_position();
    for dx in -8..=8 {
        for dz in -8..=8 {
            let at = Vector2::new(chunk.x + dx, chunk.y + dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    let villager = spawn(&world)?;
    let mut out = shipped(&villager);

    let scenes = synthetic();
    write_pack(
        &server
            .basic_config
            .get_world_path()
            .join("datapacks/trades"),
        &scenes,
    )?;
    let mut enabled = server.level_info.load().data_packs.enabled.clone();
    enabled.push("file/trades".to_owned());
    server.datapack_manager.load_all(
        &server.basic_config.get_world_path(),
        &enabled,
        &server.recipe_manager,
    );
    out.push_str(&synthetic_text(&villager, &scenes));
    server.stop();
    script_golden::bless_or_compare("villager_trades.txt", &out)
}
