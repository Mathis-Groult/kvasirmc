//! A record of what `FlyNodeEvaluator::get_neighbors` returns, made before it is rewritten
//! (`tests/golden/fly_neighbors.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! A real world gets a block pattern chosen by a hash of each position (air, stone, water,
//! lava, fence, cactus). For each of two mobs and a grid of centers, the evaluator is
//! prepared afresh, some nodes are closed beforehand, and `get_neighbors` is asked for the
//! centre; the nodes it returns (position, path type, malus) are listed in order.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::entity::EntityType;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::pathfinder::fly_node_evaluator::FlyNodeEvaluator;
use crate::entity::ai::pathfinder::node::Node;
use crate::entity::ai::pathfinder::node_evaluator::{MobData, NodeEvaluator};
use crate::entity::ai::pathfinder::pathfinding_context::PathfindingContext;
use crate::entity::ai::test_support::world;
use crate::entity::living::LivingEntity;
use crate::entity::Entity;

const LOW: i32 = 60;
const HIGH: i32 = 68;
const SIZE: i32 = 8;

fn hash(x: i32, y: i32, z: i32, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1)
        ^ (y as u32).wrapping_mul(0x85eb_ca6b)
        ^ (z as u32).wrapping_mul(0xc2b2_ae35)
        ^ salt.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 13)
}

fn block_at(x: i32, y: i32, z: i32, salt: u32) -> Option<&'static Block> {
    match hash(x, y, z, salt) % 16 {
        0..=1 => Some(&Block::STONE),
        2 => Some(&Block::WATER),
        3 => Some(&Block::LAVA),
        4 => Some(&Block::OAK_FENCE),
        5 => Some(&Block::CACTUS),
        _ => None,
    }
}

/// Nodes are closed for roughly one position in five.
fn closed_at(x: i32, y: i32, z: i32) -> bool {
    hash(x, y, z, 99).is_multiple_of(5)
}

fn node_text(node: &Node) -> String {
    format!(
        "{},{},{}:{:?}:{}",
        node.pos.0.x, node.pos.0.y, node.pos.0.z, node.path_type, node.cost_malus
    )
}

fn neighbors_text(
    world: &Arc<crate::world::World>,
    living: &LivingEntity,
    center: (i32, i32, i32),
    close: bool,
) -> String {
    let data = MobData::from_entity(living, world);
    let mut evaluator = FlyNodeEvaluator::new();
    evaluator.prepare(
        PathfindingContext::new(BlockPos::floored_v(data.position).0, world.clone()),
        data,
    );
    if close {
        for x in center.0 - 1..=center.0 + 1 {
            for y in center.1 - 1..=center.1 + 1 {
                for z in center.2 - 1..=center.2 + 1 {
                    if closed_at(x, y, z) {
                        let mut node = evaluator.walk.base.get_node(BlockPos::new(x, y, z));
                        node.closed = true;
                        evaluator.walk.base.update_node(node);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    evaluator.get_neighbors(
        &Node::new(BlockPos::new(center.0, center.1, center.2)),
        &mut out,
    );
    let text = out.iter().map(node_text).collect::<Vec<_>>().join(" ");
    evaluator.done();
    format!("{} [{text}]", out.len())
}

#[tokio::test(flavor = "multi_thread")]
async fn fly_neighbors() -> TestResult {
    let folder = tempfile::tempdir()?;
    let world = world(&folder);
    let mut report = String::new();
    for (name, entity_type) in [("bat", &EntityType::BAT), ("ghast", &EntityType::GHAST)] {
        for salt in 0..3u32 {
            let chunk = pumpkin_world::chunk::ChunkData::empty_sync(0, 0);
            for x in 0..SIZE {
                for z in 0..SIZE {
                    for y in LOW..HIGH {
                        if let Some(block) = block_at(x, y, z, salt) {
                            chunk.set_block_absolute_y(
                                x as usize,
                                y,
                                z as usize,
                                block.default_state.id,
                            );
                        }
                    }
                }
            }
            world.level.loaded_chunks.insert(Vector2::new(0, 0), chunk);
            let living = LivingEntity::new(Entity::new(
                world.clone(),
                Vector3::new(0.5, 64.0, 0.5),
                entity_type,
            ));
            for x in 1..SIZE - 1 {
                for y in LOW + 1..HIGH - 1 {
                    for z in 1..SIZE - 1 {
                        if (x + y + z) % 3 != salt as i32 {
                            continue;
                        }
                        for close in [false, true] {
                            let text = neighbors_text(&world, &living, (x, y, z), close);
                            let _ = writeln!(
                                report,
                                "{name} {salt} {x},{y},{z} closed {close} -> {text}"
                            );
                        }
                    }
                }
            }
        }
    }
    world.level.shutdown().await;
    script_golden::bless_or_compare("fly_neighbors.txt", &report)
}
