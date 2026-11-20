//! `/effect` on mobs (vanilla `EffectCommands`: the targets are any entities, and an entity
//! without effects, such as an item, is skipped but still counted in the feedback).

use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;

const ZOMBIE: &str = "@e[type=minecraft:zombie]";

fn speed_of(server: &crate::server::Server) -> Vec<Option<u8>> {
    let world = server.worlds.load()[0].clone();
    world
        .entities
        .load()
        .iter()
        .filter(|entity| entity.get_entity().entity_type.id == EntityType::ZOMBIE.id)
        .map(|entity| {
            entity
                .get_living_entity()
                .and_then(|living| living.get_effect(&StatusEffect::SPEED))
                .map(|effect| effect.amplifier)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn effects_can_be_given_to_and_cleared_from_mobs() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let online = script_golden::join(&world).await?;
    let source = online[0].player.clone();

    let summoned = script_golden::run(&server, &source, "summon minecraft:zombie 0 64 0");
    assert!(summoned.starts_with("ok"), "{summoned}");
    let none = speed_of(&server);

    let give = script_golden::run(
        &server,
        &source,
        &format!("effect give {ZOMBIE} minecraft:speed 30 2"),
    );
    let given = speed_of(&server);
    let again = script_golden::run(
        &server,
        &source,
        &format!("effect give {ZOMBIE} minecraft:speed 30 1"),
    );
    let clear = script_golden::run(
        &server,
        &source,
        &format!("effect clear {ZOMBIE} minecraft:speed"),
    );
    let cleared = speed_of(&server);
    let nothing = script_golden::run(&server, &source, &format!("effect clear {ZOMBIE}"));

    assert_eq!(none, [None]);
    assert!(give.starts_with("ok"), "{give}");
    assert_eq!(given, [Some(2)]);
    assert!(
        again.starts_with("err"),
        "a weaker effect does not replace: {again}"
    );
    assert!(clear.starts_with("ok"), "{clear}");
    assert_eq!(cleared, [None]);
    assert!(nothing.starts_with("err"), "{nothing}");
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
