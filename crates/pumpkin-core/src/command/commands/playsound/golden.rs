//! A record of `/playsound` run through the shared script harness
//! (`tests/golden/playsound_command.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made before
//! `PlaySoundExecutor::execute` and `play_sound` were split.
//!
//! The source `GoldA` stands at the command position (0.5, 64, 0.5); `GoldB` 10 blocks away,
//! `GoldC` 40 blocks away and `GoldD` 300 blocks away, so the hearing range (16 blocks times the
//! volume when it is over 1) and the minimum-volume fallback are both reached. The sound packet
//! carries a random seed: its last 8 bytes are blanked before hashing.

use std::fmt::Write;

use pumpkin_util::math::vector3::Vector3;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;

const STEPS: &[&str] = &[
    // Sound only, then each category.
    "playsound minecraft:entity.pig.ambient",
    "playsound entity.pig.ambient",
    "playsound minecraft:custom.not_a_vanilla_sound",
    "playsound minecraft:entity.pig.ambient master",
    "playsound minecraft:entity.pig.ambient music",
    "playsound minecraft:entity.pig.ambient record",
    "playsound minecraft:entity.pig.ambient weather",
    "playsound minecraft:entity.pig.ambient block",
    "playsound minecraft:entity.pig.ambient hostile",
    "playsound minecraft:entity.pig.ambient neutral",
    "playsound minecraft:entity.pig.ambient player",
    "playsound minecraft:entity.pig.ambient ambient",
    "playsound minecraft:entity.pig.ambient voice",
    // Targets.
    "playsound minecraft:block.note_block.harp block GoldB",
    "playsound minecraft:block.note_block.harp block GoldC",
    "playsound minecraft:block.note_block.harp block @a",
    "playsound minecraft:block.note_block.harp block @a[name=!GoldA]",
    // Position.
    "playsound minecraft:block.note_block.harp block @a 10.5 64 0.5",
    "playsound minecraft:block.note_block.harp block @a 40.5 64 0.5",
    "playsound minecraft:block.note_block.harp block @a ~ ~10 ~",
    "playsound minecraft:block.note_block.harp block GoldD 0.5 64 0.5",
    // Volume: range 16 x volume above 1.
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 0.5",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 1",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 2",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 2.5",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 20",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 0",
    // Pitch.
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 1 0",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 1 0.5",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 1 2",
    // Minimum volume for listeners out of range.
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 1 1 0",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 1 1 0.25",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 0.5 1 1 1",
    "playsound minecraft:block.note_block.harp block GoldD 0.5 64 0.5 1 1 0.5",
    "playsound minecraft:block.note_block.harp block GoldD 0.5 64 0.5 1 1 0",
    "playsound minecraft:block.note_block.harp block GoldD 0.5 64 0.5 30 1 0",
    // Malformed.
    "playsound",
    "playsound minecraft:entity.pig.ambient loud",
    "playsound minecraft:entity.pig.ambient block @a 0.5 64 0.5 -1",
    "playsound minecraft:entity.pig.ambient block @a 0.5 64 0.5 1 3",
    "playsound minecraft:entity.pig.ambient block @a 0.5 64 0.5 1 1 2",
    "playsound minecraft:entity.pig.ambient block NoSuchPlayer",
    // Appended after the mutation run: listeners offset in y and z too, at the range boundary
    // and out of range with a minimum volume.
    "playsound minecraft:block.note_block.harp block @a 6.5 70 8.5",
    "playsound minecraft:block.note_block.harp block @a 0.5 80 0.5",
    "playsound minecraft:block.note_block.harp block @a 0.5 64 16.5",
    "playsound minecraft:block.note_block.harp block @a 0.5 100 30.5 1 1 0.5",
    "playsound minecraft:block.note_block.harp block @a 300.5 64 -50.5 1 1 0.5",
];

#[tokio::test(flavor = "multi_thread")]
async fn playsound_command_sends_the_same_packets() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    for (who, x) in online.iter().zip([0.5, 10.5, 40.5, 300.5]) {
        who.player
            .living_entity
            .entity
            .set_pos(Vector3::new(x, 64.0, 0.5));
    }
    script_golden::packets(&mut online);
    let source_player = online[0].player.clone();

    let mut now = String::new();
    for step in STEPS {
        let result = script_golden::run(&server, &source_player, step);
        let sent = script_golden::packets_normalised(&mut online, &script_golden::blank_seed);
        writeln!(now, "{step} -> {result};{sent}")?;
    }

    script_golden::bless_or_compare("playsound_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
