use crate::command::node::dispatcher::CommandDispatcher;
use pumpkin_config::CommandsConfig;
use pumpkin_util::{
    PermissionLvl,
    permission::{Permission, PermissionDefault, PermissionManager, PermissionRegistry},
};
use tracing::{info, warn};

mod advancement;
mod attribute;
mod ban;
mod banip;
mod banlist;
mod bossbar;
mod clear;
mod clone;
mod compute;
mod damage;
mod data;
mod datapack;
mod debug;
pub mod defaultgamemode;
mod deop;
mod dialog;
mod difficulty;
mod effect;
mod enchant;
mod execute;
mod experience;
mod fetchprofile;
mod fill;
mod fillbiome;
mod forceload;
mod function;
mod gamemode;
mod gamerule;
mod give;
mod help;
mod item;
mod kick;
mod kill;
mod list;
mod locate;
mod loot;
mod me;
mod msg;
mod op;
mod pardon;
mod pardonip;
mod particle;
mod place;
mod playsound;
mod plugin;
mod plugins;
mod posteffect;
mod pregen;
mod pumpkin;
mod raid;
mod random;
mod recipe;
mod reload;
mod r#return;
mod ride;
mod rotate;
mod saveall;
mod saveoff;
mod saveon;
mod say;
mod schedule;
mod scoreboard;
mod seed;
mod setblock;
mod setidletimeout;
mod setworldspawn;
mod spawnpoint;
mod spectate;
mod spreadplayers;
mod stop;
mod stopsound;
mod stopwatch;
mod summon;
mod swing;
mod tag;
mod team;
mod teammsg;
mod teleport;
mod tellraw;
mod test;
mod tick;
mod tickprofile;
mod time;
mod title;
mod tps;
mod transfer;
#[cfg(test)]
mod script_golden;
#[cfg(test)]
mod tree_golden;
mod trigger;
mod waypoint;
mod weather;
mod whitelist;
pub(crate) use whitelist::kick_non_whitelisted_players;
mod worldborder;

/// Every built-in command's registration, in the order the dispatcher receives them.
const REGISTRATIONS: [Registration; 91] = [
    say::register,
    banlist::register,
    difficulty::register,
    debug::register,
    dialog::register,
    execute::register,
    fillbiome::register,
    forceload::register,
    pregen::register,
    tickprofile::register,
    ride::register,
    recipe::register,
    reload::register,
    help::register,
    kill::register,
    op::register,
    place::register,
    random::register,
    list::register,
    locate::register,
    loot::register,
    seed::register,
    saveall::register,
    saveoff::register,
    saveon::register,
    setidletimeout::register,
    spreadplayers::register,
    stop::register,
    tag::register,
    tick::register,
    advancement::register,
    data::register,
    stopwatch::register,
    trigger::register,
    scoreboard::register,
    test::register,
    team::register,
    teammsg::register,
    clone::register,
    attribute::register,
    datapack::register,
    function::register,
    r#return::register,
    schedule::register,
    fetchprofile::register,
    worldborder::register,
    particle::register,
    playsound::register,
    fill::register,
    clear::register,
    pumpkin::register,
    me::register,
    msg::register,
    tps::register,
    transfer::register,
    gamemode::register,
    defaultgamemode::register,
    weather::register,
    time::register,
    teleport::register,
    setworldspawn::register,
    spawnpoint::register,
    spectate::register,
    setblock::register,
    give::register,
    item::register,
    enchant::register,
    effect::register,
    summon::register,
    swing::register,
    compute::register,
    posteffect::register,
    damage::register,
    rotate::register,
    tellraw::register,
    title::register,
    experience::register,
    bossbar::register,
    gamerule::register,
    stopsound::register,
    waypoint::register,
    raid::register,
    deop::register,
    kick::register,
    plugin::register,
    plugins::register,
    ban::register,
    banip::register,
    pardon::register,
    pardonip::register,
    whitelist::register,
];

/// Adds one command (and its permission) to the dispatcher.
type Registration = fn(&mut CommandDispatcher, &PermissionRegistry);

#[must_use]
pub fn default_dispatcher(
    permission_manager: &PermissionManager,
    commands_config: &CommandsConfig,
) -> CommandDispatcher {
    let registry = &permission_manager.registry;

    register_permissions(registry);

    let mut dispatcher = CommandDispatcher::new();

    for register in REGISTRATIONS {
        register(&mut dispatcher, registry);
    }

    apply_command_overrides(&mut dispatcher, registry, commands_config);

    dispatcher
}

/// Applies the per-command settings from the server configuration on top of the
/// freshly built dispatcher.
///
/// Two kinds of override are supported:
/// - Disabling a command, which removes it from the legacy dispatcher and marks
///   its name so the wrapper dispatcher hides it everywhere else.
/// - Changing a command's required permission level, which is done by rewriting
///   the default of its permission node in the registry. Because command
///   requirements look their permission up in the registry at execution time,
///   this affects both the legacy and the node-based dispatchers uniformly.
fn apply_command_overrides(
    dispatcher: &mut CommandDispatcher,
    registry: &PermissionRegistry,
    commands_config: &CommandsConfig,
) {
    for (raw_name, settings) in &commands_config.overrides {
        // Command names are always lowercase, so normalise here to be forgiving
        // of how the owner wrote them in the config file.
        let name = raw_name.to_ascii_lowercase();

        // Catch typos: an override for a command that does not exist almost
        // always means the owner misspelled it, so tell them instead of silently
        // doing nothing.
        if !dispatcher.has_command(&name) {
            warn!(
                "Ignoring the command setting for \"{raw_name}\" because there is no command with that name (check the spelling in your config)"
            );
            continue;
        }

        if !settings.enabled {
            // If the owner named an alias (e.g. `tp` for `teleport`), turn off
            // the whole command, not just that one alias.
            let primary = dispatcher.primary_command_name(&name);

            dispatcher.disable_command(name.clone());
            dispatcher.disable_command(primary.clone());
            // Node-based commands keep their aliases as redirecting root nodes,
            // so flag those too.
            for alias in dispatcher.tree_alias_names(&primary) {
                dispatcher.disable_command(alias);
            }
            info!("The /{primary} command has been turned off in the configuration");
            // A disabled command can never be run, so its permission level is
            // irrelevant; skip the rest.
            continue;
        }

        if let Some(level) = settings.permission_level {
            let default = if level == PermissionLvl::Zero {
                PermissionDefault::Allow
            } else {
                PermissionDefault::Op(level)
            };

            if let Some(node) = resolve_permission_node(dispatcher, registry, &name) {
                if registry.set_default(&node, default) {
                    info!(
                        "The /{name} command now needs permission level {} to use",
                        level as u8
                    );
                } else {
                    warn!(
                        "Command override for /{name} sets a permission level, but matching permission node could not be updated; leaving it unchanged"
                    );
                }
            } else {
                warn!(
                    "Command override for /{name} sets a permission level, but no matching permission node could be found; leaving it unchanged"
                );
            }
        }
    }
}

/// Finds the permission node associated with a command name.
///
/// Follows the `<namespace>:command.<name>` convention by probing the registry.
fn resolve_permission_node(
    _dispatcher: &CommandDispatcher,
    registry: &PermissionRegistry,
    name: &str,
) -> Option<String> {
    for namespace in ["minecraft", "pumpkin"] {
        let candidate = format!("{namespace}:command.{name}");
        if registry.get_permission(&candidate).is_some() {
            return Some(candidate);
        }
    }

    None
}

fn register_permissions(registry: &PermissionRegistry) {
    // Register our entity selector permission as well.
    registry
        .register_permission(Permission::new(
            "minecraft:command.selector",
            "Allows a player to use selector variables",
            PermissionDefault::Allow,
        ))
        .unwrap_or_else(|e| tracing::warn!("{e}"));
    // Checked with vanilla's isOp mapped to level One, see `NBT_PLACE_PERMISSION`.
    registry
        .register_permission(Permission::new(
            crate::item::items::spawn_egg::NBT_PLACE_PERMISSION,
            "Allows a player to place op-only entity data from spawn eggs",
            PermissionDefault::Op(PermissionLvl::One),
        ))
        .unwrap_or_else(|e| tracing::warn!("{e}"));
}

#[cfg(test)]
mod override_tests {
    use pumpkin_config::{CommandOverride, CommandsConfig};
    use pumpkin_util::PermissionLvl;
    use pumpkin_util::permission::{PermissionDefault, PermissionManager};

    use super::default_dispatcher;

    fn disabled(config: &mut CommandsConfig, name: &str) {
        config.overrides.insert(
            name.to_string(),
            CommandOverride {
                enabled: false,
                permission_level: None,
            },
        );
    }

    fn permission(config: &mut CommandsConfig, name: &str, level: PermissionLvl) {
        config.overrides.insert(
            name.to_string(),
            CommandOverride {
                enabled: true,
                permission_level: Some(level),
            },
        );
    }

    #[test]
    fn disabling_a_command_removes_and_hides_it() {
        let mut commands = CommandsConfig::default();
        disabled(&mut commands, "gamemode");

        let manager = PermissionManager::new();
        let dispatcher = default_dispatcher(&manager, &commands);

        assert!(dispatcher.is_disabled("gamemode"));
        assert!(
            !dispatcher.is_disabled("give"),
            "untouched commands stay on"
        );
    }

    #[test]
    fn disabling_an_alias_turns_off_the_whole_command() {
        let mut commands = CommandsConfig::default();
        // `tp` is an alias of `teleport`; disabling it should take the whole
        // command down, including the primary name.
        disabled(&mut commands, "tp");

        let manager = PermissionManager::new();
        let dispatcher = default_dispatcher(&manager, &commands);

        assert!(dispatcher.is_disabled("tp"));
        assert!(dispatcher.is_disabled("teleport"));
    }

    #[test]
    fn disabling_a_node_command_also_disables_its_aliases() {
        let mut commands = CommandsConfig::default();
        disabled(&mut commands, "experience");

        let manager = PermissionManager::new();
        let dispatcher = default_dispatcher(&manager, &commands);

        assert!(dispatcher.is_disabled("experience"));
        assert!(dispatcher.is_disabled("xp"));
    }

    #[test]
    fn override_for_unknown_command_is_ignored() {
        let mut commands = CommandsConfig::default();
        // A command name that does not exist (usually a typo in the config)
        // should be ignored, not silently swallow a real command or panic.
        disabled(&mut commands, "notacommand");

        let manager = PermissionManager::new();
        let dispatcher = default_dispatcher(&manager, &commands);

        assert!(!dispatcher.is_disabled("notacommand"));
        assert!(dispatcher.has_command("gamemode"));
    }

    #[test]
    fn override_is_case_insensitive() {
        let mut commands = CommandsConfig::default();
        disabled(&mut commands, "GameMode");

        let manager = PermissionManager::new();
        let dispatcher = default_dispatcher(&manager, &commands);

        assert!(dispatcher.is_disabled("gamemode"));
    }

    #[test]
    fn permission_level_override_rewrites_the_registry_default() {
        let mut commands = CommandsConfig::default();
        // `gamemode` is normally level 2; bump it to owner-only.
        permission(&mut commands, "gamemode", PermissionLvl::Four);

        let manager = PermissionManager::new();
        let _dispatcher = default_dispatcher(&manager, &commands);

        let permission = manager
            .get_permission("minecraft:command.gamemode")
            .expect("gamemode permission should be registered");
        assert_eq!(
            permission.default,
            PermissionDefault::Op(PermissionLvl::Four)
        );
    }

    #[test]
    fn permission_override_resolves_node_command_by_convention() {
        let mut commands = CommandsConfig::default();
        // `kill` is a node-based command whose permission node is not recorded in
        // the legacy dispatcher, so the override must fall back to the
        // `minecraft:command.kill` naming convention.
        permission(&mut commands, "kill", PermissionLvl::Four);

        let manager = PermissionManager::new();
        let _dispatcher = default_dispatcher(&manager, &commands);

        let permission = manager
            .get_permission("minecraft:command.kill")
            .expect("kill permission should be registered");
        assert_eq!(
            permission.default,
            PermissionDefault::Op(PermissionLvl::Four)
        );
    }

    #[test]
    fn permission_level_zero_allows_everyone() {
        let mut commands = CommandsConfig::default();
        permission(&mut commands, "gamemode", PermissionLvl::Zero);

        let manager = PermissionManager::new();
        let _dispatcher = default_dispatcher(&manager, &commands);

        let permission = manager
            .get_permission("minecraft:command.gamemode")
            .expect("gamemode permission should be registered");
        assert_eq!(permission.default, PermissionDefault::Allow);
    }

    #[test]
    fn version_is_an_alias_of_pumpkin() {
        let manager = PermissionManager::new();
        let dispatcher = default_dispatcher(&manager, &CommandsConfig::default());

        assert!(dispatcher.has_command("version"));
        assert_eq!(dispatcher.primary_command_name("version"), "pumpkin");
        assert!(!dispatcher.has_command("ver"));
    }
}
