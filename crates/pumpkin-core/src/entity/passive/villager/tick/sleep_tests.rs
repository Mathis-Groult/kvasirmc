//! Villagers go to bed every night, not only on the first day (card #83): the night test reads
//! the time of day within the day, as the world's time of day keeps growing past 24000.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::Block;
use pumpkin_data::block_properties::{BedPart, HorizontalFacing, WhiteBedLikeProperties};
use pumpkin_data::entity::{EntityPose, EntityType};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::entity::ai::test_support;
use crate::entity::passive::villager::VillagerEntity;
use crate::entity::{Entity, EntityBase};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Whether a villager standing beside its free bed is asleep after one tick at `time_of_day`.
async fn sleeps_at(time_of_day: i64) -> Result<bool, Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for dx in -2..=2 {
        for dz in -2..=2 {
            let at = Vector2::new(dx, dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    let head = BlockPos::new(9, 64, 8);
    let mut bed = WhiteBedLikeProperties::default(&Block::RED_BED);
    bed.facing = HorizontalFacing::North;
    bed.part = BedPart::Foot;
    world.set_block_state(
        &head.offset(HorizontalFacing::South.to_offset()),
        bed.to_state_id(&Block::RED_BED),
        BlockFlags::NOTIFY_LISTENERS,
    );
    bed.part = BedPart::Head;
    world.set_block_state(
        &head,
        bed.to_state_id(&Block::RED_BED),
        BlockFlags::NOTIFY_LISTENERS,
    );
    let villager = VillagerEntity::new(Entity::new(
        world.clone(),
        Vector3::new(8.5, 64.0, 8.5),
        &EntityType::VILLAGER,
    ));
    world.spawn_entity(villager.clone() as Arc<dyn EntityBase>);
    *villager
        .home_pos
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(head);
    villager
        .get_entity()
        .tick_count
        .store(40, Ordering::Relaxed);
    world
        .level_time
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .time_of_day = time_of_day;
    villager.villager_mob_tick();
    let asleep = villager.get_entity().pose.load() == EntityPose::Sleeping;
    server.stop();
    Ok(asleep)
}

#[tokio::test(flavor = "multi_thread")]
async fn sleeps_on_the_first_night() -> TestResult {
    assert!(sleeps_at(13_000).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn sleeps_on_the_second_night() -> TestResult {
    assert!(sleeps_at(24_000 + 13_000).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn stays_up_on_the_second_day() -> TestResult {
    assert!(!sleeps_at(24_000 + 6_000).await?);
    Ok(())
}
