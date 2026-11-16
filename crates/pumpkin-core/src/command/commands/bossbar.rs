use uuid::Uuid;

use pumpkin_data::translation;
use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::hover::HoverEvent;

use crate::command::CommandSource;
use crate::command::argument_builder::{ArgumentBuilder, argument, command, literal};
use crate::command::argument_types::FromStringReader;
use crate::command::argument_types::argument_type::{ArgumentType, JavaClientArgumentType};
use crate::command::argument_types::component::ComponentArgumentType;
use crate::command::argument_types::core::bool::BoolArgumentType;
use crate::command::argument_types::core::integer::IntegerArgumentType;
use crate::command::argument_types::entity::EntityArgumentType;
use crate::command::argument_types::identifier::IdentifierArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::errors::command_syntax_error::CommandSyntaxError;
use crate::command::errors::error_types::CommandErrorType;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use crate::command::string_reader::StringReader;
use crate::command::suggestion::suggestions::{Suggestions, SuggestionsBuilder};
use crate::server::Server;
use crate::world::bossbar::{Bossbar, BossbarColor, BossbarDivisions};
use crate::world::custom_bossbar::{BossbarUpdateError, CustomBossbar};

const DESCRIPTION: &str = "Creates and modifies boss bars";
const PERMISSION: &str = "minecraft:command.bossbar";

const ERROR_UNKNOWN_BOSSBAR: CommandErrorType<1> = CommandErrorType::new(
    translation::java::COMMANDS_BOSSBAR_UNKNOWN,
    translation::bedrock::COMMANDS_BOSSBAR_NOTFOUND,
);

const ERROR_CREATE_FAILED: CommandErrorType<1> = CommandErrorType::new(
    translation::java::COMMANDS_BOSSBAR_CREATE_FAILED,
    translation::bedrock::COMMANDS_BOSSBAR_ADD_FAILURE_EXISTS,
);

#[derive(Clone, Copy)]
pub struct BossbarIdArgumentType;

impl ArgumentType<CommandSource> for BossbarIdArgumentType {
    type Item = String;

    fn parse(&self, reader: &mut StringReader) -> Result<Self::Item, CommandSyntaxError> {
        let ident = pumpkin_util::identifier::Identifier::from_reader(reader)?;
        Ok(ident.to_string())
    }

    fn client_side_parser(&'_ self) -> JavaClientArgumentType {
        JavaClientArgumentType::ResourceLocation
    }

    fn list_suggestions(
        &self,
        context: &CommandContext,
        builder: SuggestionsBuilder,
    ) -> Suggestions {
        let bossbars = context.source.server().bossbars.lock().unwrap();
        builder
            .filter_and_suggest_iter(bossbars.custom_bossbars.keys().cloned())
            .build()
    }
}

impl BossbarIdArgumentType {
    pub fn get(context: &CommandContext, name: &str) -> Result<String, CommandSyntaxError> {
        context.get_argument::<String>(name).cloned()
    }
}

/// The colours `bossbar set <id> color` lists, as literals.
const COLORS: [&str; 7] = ["blue", "green", "pink", "purple", "red", "white", "yellow"];

/// The styles `bossbar set <id> style` lists, as literals.
const STYLES: [&str; 5] = [
    "notched_10",
    "notched_12",
    "notched_20",
    "notched_6",
    "progress",
];

/// `BossEvent.BossBarColor` by its name.
fn parse_color(name: &str) -> Option<BossbarColor> {
    Some(match name {
        "blue" => BossbarColor::Blue,
        "green" => BossbarColor::Green,
        "pink" => BossbarColor::Pink,
        "purple" => BossbarColor::Purple,
        "red" => BossbarColor::Red,
        "white" => BossbarColor::White,
        "yellow" => BossbarColor::Yellow,
        _ => return None,
    })
}

/// `BossEvent.BossBarOverlay` by its name.
fn parse_style(name: &str) -> Option<BossbarDivisions> {
    Some(match name {
        "notched_10" => BossbarDivisions::Notches10,
        "notched_12" => BossbarDivisions::Notches12,
        "notched_20" => BossbarDivisions::Notches20,
        "notched_6" => BossbarDivisions::Notches6,
        "progress" => BossbarDivisions::NoDivision,
        _ => return None,
    })
}

fn bossbar_prefix(title: TextComponent, namespace: String) -> TextComponent {
    TextComponent::text("[")
        .add_child(title)
        .add_child(TextComponent::text("]"))
        .hover_event(HoverEvent::show_text(TextComponent::text(namespace)))
}

fn handle_bossbar_error(error: BossbarUpdateError) -> CommandSyntaxError {
    match error {
        BossbarUpdateError::InvalidResourceLocation(location) => {
            ERROR_UNKNOWN_BOSSBAR.create_without_context(TextComponent::text(location))
        }
        BossbarUpdateError::NoChanges(value, variation) => {
            let key = variation.map_or_else(
                || format!("commands.bossbar.set.{value}.unchanged"),
                |var| format!("commands.bossbar.set.{value}.unchanged.{var}"),
            );

            crate::command::errors::error_types::DISPATCHER_PARSE_EXCEPTION
                .create_without_context(TextComponent::translate_cross(key.clone(), key, []))
        }
    }
}

enum CommandValueGet {
    Max,
    Players,
    Value,
    Visible,
}

enum CommandValueSet {
    Color(BossbarColor),
    Max,
    Name,
    Players(bool),
    Style(BossbarDivisions),
    Value,
    Visible,
}

struct AddExecutor;

impl CommandExecutor for AddExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let namespace = IdentifierArgumentType::get(context, "id")?.to_string();
        let text_component = ComponentArgumentType::get(context, "name")?;
        let server = context.source.server();

        if server.bossbars.lock().unwrap().has_bossbar(&namespace) {
            return Err(ERROR_CREATE_FAILED.create_without_context(TextComponent::text(namespace)));
        }

        let bossbar = Bossbar::new(text_component);
        let mut bossbars = server.bossbars.lock().unwrap();

        bossbars.create_bossbar(namespace.clone(), bossbar.clone());
        let new_size = bossbars.get_bossbars_len();
        drop(bossbars);

        context.source.send_feedback(
            TextComponent::translate_cross(
                translation::java::COMMANDS_BOSSBAR_CREATE_SUCCESS,
                translation::bedrock::COMMANDS_BOSSBAR_ADD_SUCCESS,
                [bossbar_prefix(bossbar.title, namespace)],
            ),
            true,
        );

        Ok(new_size as i32)
    }
}

struct GetExecutor(CommandValueGet);

impl CommandExecutor for GetExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let namespace = BossbarIdArgumentType::get(context, "id")?;
        let server = context.source.server();

        let Some(bossbar) = server.bossbars.lock().unwrap().get_bossbar(&namespace) else {
            return Err(handle_bossbar_error(
                BossbarUpdateError::InvalidResourceLocation(namespace),
            ));
        };

        match self.0 {
            CommandValueGet::Max => Ok(get_max(context, namespace, &bossbar)),
            CommandValueGet::Players => Ok(get_players(context, server, namespace, bossbar)),
            CommandValueGet::Value => Ok(get_value(context, namespace, &bossbar)),
            CommandValueGet::Visible => Ok(get_visible(context, namespace, &bossbar)),
        }
    }
}

struct ListExecutor;

impl CommandExecutor for ListExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let server = context.source.server();
        let bossbars = server.bossbars.lock().unwrap().get_all_bossbars();

        if bossbars.is_empty() {
            context.source.send_feedback(
                TextComponent::translate_cross(
                    translation::java::COMMANDS_BOSSBAR_LIST_BARS_NONE,
                    translation::bedrock::COMMANDS_BOSSBAR_LIST_NONE,
                    [],
                ),
                false,
            );
            return Ok(0);
        }

        let mut bossbars_text = TextComponent::empty();
        for (i, bossbar) in bossbars.iter().enumerate() {
            if i == 0 {
                bossbars_text = bossbars_text.add_child(bossbar_prefix(
                    bossbar.bossbar_data.title.clone(),
                    bossbar.namespace.clone(),
                ));
            } else {
                bossbars_text =
                    bossbars_text.add_child(TextComponent::text(", ").add_child(bossbar_prefix(
                        bossbar.bossbar_data.title.clone(),
                        bossbar.namespace.clone(),
                    )));
            }
        }

        context.source.send_feedback(
            TextComponent::translate_cross(
                translation::java::COMMANDS_BOSSBAR_LIST_BARS_SOME,
                translation::bedrock::COMMANDS_BOSSBAR_LIST,
                [
                    TextComponent::text(bossbars.len().to_string()),
                    bossbars_text,
                ],
            ),
            false,
        );

        Ok(bossbars.len() as i32)
    }
}

struct RemoveExecutor;

impl CommandExecutor for RemoveExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let namespace = BossbarIdArgumentType::get(context, "id")?;
        let server = context.source.server();

        let Some(bossbar) = server.bossbars.lock().unwrap().get_bossbar(&namespace) else {
            return Err(handle_bossbar_error(
                BossbarUpdateError::InvalidResourceLocation(namespace),
            ));
        };

        context.source.send_feedback(
            TextComponent::translate_cross(
                translation::java::COMMANDS_BOSSBAR_REMOVE_SUCCESS,
                translation::bedrock::COMMANDS_BOSSBAR_REMOVE,
                [bossbar_prefix(
                    bossbar.bossbar_data.title,
                    namespace.clone(),
                )],
            ),
            true,
        );

        let res = server
            .bossbars
            .lock()
            .unwrap()
            .remove_bossbar(server, namespace);
        match res {
            Ok(()) => Ok(server.bossbars.lock().unwrap().get_bossbars_len() as i32),
            Err(error) => Err(handle_bossbar_error(error)),
        }
    }
}

struct SetExecutor(CommandValueSet);

impl CommandExecutor for SetExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let namespace = BossbarIdArgumentType::get(context, "id")?;
        let server = context.source.server();

        let Some(bossbar) = server.bossbars.lock().unwrap().get_bossbar(&namespace) else {
            return Err(handle_bossbar_error(
                BossbarUpdateError::InvalidResourceLocation(namespace),
            ));
        };

        match self.0 {
            CommandValueSet::Color(color) => set_color(context, server, namespace, bossbar, color),
            CommandValueSet::Max => set_max(context, server, namespace, bossbar),
            CommandValueSet::Name => set_name(context, server, namespace),
            CommandValueSet::Players(has_players) => {
                set_players(context, server, namespace, bossbar, has_players)
            }
            CommandValueSet::Style(style) => set_style(context, server, namespace, bossbar, style),
            CommandValueSet::Value => set_value(context, server, namespace, bossbar),
            CommandValueSet::Visible => set_visible(context, server, namespace, bossbar),
        }
    }
}

/// `bossbar get <id> max`.
fn get_max(context: &CommandContext, namespace: String, bossbar: &CustomBossbar) -> i32 {
    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_GET_MAX,
            translation::bedrock::COMMANDS_BOSSBAR_GET_MAX,
            [
                bossbar_prefix(bossbar.bossbar_data.title.clone(), namespace),
                TextComponent::text(bossbar.max.to_string()),
            ],
        ),
        true,
    );
    bossbar.max
}

/// `bossbar get <id> players`.
fn get_players(
    context: &CommandContext,
    server: &Server,
    namespace: String,
    bossbar: CustomBossbar,
) -> i32 {
    let online_players: Vec<String> = server
        .get_all_players()
        .iter()
        .filter(|player| bossbar.players.contains(&player.gameprofile.id))
        .map(|player| player.gameprofile.name.clone())
        .collect();
    let count = online_players.len() as i32;

    if count == 0 {
        context.source.send_feedback(
            TextComponent::translate_cross(
                translation::java::COMMANDS_BOSSBAR_GET_PLAYERS_NONE,
                translation::bedrock::COMMANDS_BOSSBAR_GET_PLAYERS_NONE,
                [bossbar_prefix(bossbar.bossbar_data.title, namespace)],
            ),
            true,
        );
    } else {
        context.source.send_feedback(
            TextComponent::translate_cross(
                translation::java::COMMANDS_BOSSBAR_GET_PLAYERS_SOME,
                if count == 1 {
                    translation::bedrock::COMMANDS_BOSSBAR_GET_PLAYERS_ONE
                } else {
                    translation::bedrock::COMMANDS_BOSSBAR_GET_PLAYERS
                },
                [
                    bossbar_prefix(bossbar.bossbar_data.title, namespace),
                    TextComponent::text(count.to_string()),
                    TextComponent::text(online_players.join(", ")),
                ],
            ),
            true,
        );
    }
    count
}

/// `bossbar get <id> value`.
fn get_value(context: &CommandContext, namespace: String, bossbar: &CustomBossbar) -> i32 {
    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_GET_VALUE,
            translation::bedrock::COMMANDS_BOSSBAR_GET_VALUE,
            [
                bossbar_prefix(bossbar.bossbar_data.title.clone(), namespace),
                TextComponent::text(bossbar.value.to_string()),
            ],
        ),
        true,
    );
    bossbar.value
}

/// `bossbar get <id> visible`.
fn get_visible(context: &CommandContext, namespace: String, bossbar: &CustomBossbar) -> i32 {
    let (java_key, bedrock_key) = if bossbar.visible {
        (
            translation::java::COMMANDS_BOSSBAR_GET_VISIBLE_VISIBLE,
            translation::bedrock::COMMANDS_BOSSBAR_GET_VISIBLE_TRUE,
        )
    } else {
        (
            translation::java::COMMANDS_BOSSBAR_GET_VISIBLE_HIDDEN,
            translation::bedrock::COMMANDS_BOSSBAR_GET_VISIBLE_FALSE,
        )
    };
    context.source.send_feedback(
        TextComponent::translate_cross(
            java_key,
            bedrock_key,
            [bossbar_prefix(
                bossbar.bossbar_data.title.clone(),
                namespace,
            )],
        ),
        true,
    );
    bossbar.visible as i32
}

/// `bossbar set <id> color`.
fn set_color(
    context: &CommandContext,
    server: &Server,
    namespace: String,
    bossbar: CustomBossbar,
    color: BossbarColor,
) -> CommandExecutorResult {
    server
        .bossbars
        .lock()
        .unwrap()
        .update_color(server, &namespace, color)
        .map_err(handle_bossbar_error)?;

    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_SET_COLOR_SUCCESS,
            translation::java::COMMANDS_BOSSBAR_SET_COLOR_SUCCESS,
            [bossbar_prefix(bossbar.bossbar_data.title, namespace)],
        ),
        true,
    );

    Ok(0)
}

/// `bossbar set <id> max`.
fn set_max(
    context: &CommandContext,
    server: &Server,
    namespace: String,
    bossbar: CustomBossbar,
) -> CommandExecutorResult {
    let max_value = IntegerArgumentType::get(context, "max")?;

    server
        .bossbars
        .lock()
        .unwrap()
        .update_max(server, namespace.clone(), max_value)
        .map_err(handle_bossbar_error)?;

    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_SET_MAX_SUCCESS,
            translation::java::COMMANDS_BOSSBAR_SET_MAX_SUCCESS,
            [
                bossbar_prefix(bossbar.bossbar_data.title, namespace),
                TextComponent::text(max_value.to_string()),
            ],
        ),
        true,
    );

    Ok(max_value)
}

/// `bossbar set <id> name`.
fn set_name(context: &CommandContext, server: &Server, namespace: String) -> CommandExecutorResult {
    let name = ComponentArgumentType::get(context, "name")?;
    server
        .bossbars
        .lock()
        .unwrap()
        .update_name(server, &namespace, &name)
        .map_err(handle_bossbar_error)?;

    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_SET_NAME_SUCCESS,
            translation::java::COMMANDS_BOSSBAR_SET_NAME_SUCCESS,
            [bossbar_prefix(name, namespace)],
        ),
        true,
    );

    Ok(0)
}

/// `bossbar set <id> players`.
fn set_players(
    context: &CommandContext,
    server: &Server,
    namespace: String,
    bossbar: CustomBossbar,
    has_players: bool,
) -> CommandExecutorResult {
    if !has_players {
        server
            .bossbars
            .lock()
            .unwrap()
            .set_players(server, namespace.clone(), vec![])
            .map_err(handle_bossbar_error)?;

        context.source.send_feedback(
            TextComponent::translate_cross(
                translation::java::COMMANDS_BOSSBAR_SET_PLAYERS_SUCCESS_NONE,
                translation::java::COMMANDS_BOSSBAR_SET_PLAYERS_SUCCESS_NONE,
                [bossbar_prefix(bossbar.bossbar_data.title, namespace)],
            ),
            true,
        );

        return Ok(0);
    }

    let targets = EntityArgumentType::get_players(context, "targets")?;
    let players: Vec<Uuid> = targets.iter().map(|player| player.gameprofile.id).collect();
    let count = players.len();

    server
        .bossbars
        .lock()
        .unwrap()
        .set_players(server, namespace.clone(), players)
        .map_err(handle_bossbar_error)?;

    let player_names = targets
        .iter()
        .map(|p| p.gameprofile.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_SET_PLAYERS_SUCCESS_SOME,
            translation::java::COMMANDS_BOSSBAR_SET_PLAYERS_SUCCESS_SOME,
            [
                bossbar_prefix(bossbar.bossbar_data.title, namespace),
                TextComponent::text(count.to_string()),
                TextComponent::text(player_names),
            ],
        ),
        true,
    );

    Ok(count as i32)
}

/// `bossbar set <id> style`.
fn set_style(
    context: &CommandContext,
    server: &Server,
    namespace: String,
    bossbar: CustomBossbar,
    style: BossbarDivisions,
) -> CommandExecutorResult {
    server
        .bossbars
        .lock()
        .unwrap()
        .update_style(server, &namespace, style)
        .map_err(handle_bossbar_error)?;

    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_SET_STYLE_SUCCESS,
            translation::java::COMMANDS_BOSSBAR_SET_STYLE_SUCCESS,
            [bossbar_prefix(bossbar.bossbar_data.title, namespace)],
        ),
        true,
    );
    Ok(0)
}

/// `bossbar set <id> value`.
fn set_value(
    context: &CommandContext,
    server: &Server,
    namespace: String,
    bossbar: CustomBossbar,
) -> CommandExecutorResult {
    let value = IntegerArgumentType::get(context, "value")?;

    server
        .bossbars
        .lock()
        .unwrap()
        .update_value(server, namespace.clone(), value)
        .map_err(handle_bossbar_error)?;

    context.source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_BOSSBAR_SET_VALUE_SUCCESS,
            translation::java::COMMANDS_BOSSBAR_SET_VALUE_SUCCESS,
            [
                bossbar_prefix(bossbar.bossbar_data.title, namespace),
                TextComponent::text(value.to_string()),
            ],
        ),
        true,
    );

    Ok(value)
}

/// `bossbar set <id> visible`.
fn set_visible(
    context: &CommandContext,
    server: &Server,
    namespace: String,
    bossbar: CustomBossbar,
) -> CommandExecutorResult {
    let visibility = BoolArgumentType::get(context, "visible")?;

    server
        .bossbars
        .lock()
        .unwrap()
        .update_visibility(server, namespace.clone(), visibility)
        .map_err(handle_bossbar_error)?;

    let state = if visibility {
        translation::java::COMMANDS_BOSSBAR_SET_VISIBLE_SUCCESS_VISIBLE
    } else {
        translation::java::COMMANDS_BOSSBAR_SET_VISIBLE_SUCCESS_HIDDEN
    };

    context.source.send_feedback(
        TextComponent::translate_cross(
            state,
            state,
            [bossbar_prefix(bossbar.bossbar_data.title, namespace)],
        ),
        true,
    );

    Ok(0)
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    let add_node = literal("add").then(
        argument("id", IdentifierArgumentType)
            .then(argument("name", ComponentArgumentType).executes(AddExecutor)),
    );

    let get_node = literal("get").then(
        argument("id", BossbarIdArgumentType)
            .then(literal("max").executes(GetExecutor(CommandValueGet::Max)))
            .then(literal("players").executes(GetExecutor(CommandValueGet::Players)))
            .then(literal("value").executes(GetExecutor(CommandValueGet::Value)))
            .then(literal("visible").executes(GetExecutor(CommandValueGet::Visible))),
    );

    let remove_node =
        literal("remove").then(argument("id", BossbarIdArgumentType).executes(RemoveExecutor));

    let set_node = literal("set").then(
        argument("id", BossbarIdArgumentType)
            // Vanilla lists each colour and style as a literal.
            .then(COLORS.iter().fold(literal("color"), |node, &name| {
                let color = parse_color(name).expect("listed colour");
                node.then(literal(name).executes(SetExecutor(CommandValueSet::Color(color))))
            }))
            .then(
                literal("max").then(
                    argument("max", IntegerArgumentType::with_min(1))
                        .executes(SetExecutor(CommandValueSet::Max)),
                ),
            )
            .then(
                literal("name").then(
                    argument("name", ComponentArgumentType)
                        .executes(SetExecutor(CommandValueSet::Name)),
                ),
            )
            .then(
                literal("players")
                    .executes(SetExecutor(CommandValueSet::Players(false)))
                    .then(
                        argument("targets", EntityArgumentType::Players)
                            .executes(SetExecutor(CommandValueSet::Players(true))),
                    ),
            )
            .then(STYLES.iter().fold(literal("style"), |node, &name| {
                let style = parse_style(name).expect("listed style");
                node.then(literal(name).executes(SetExecutor(CommandValueSet::Style(style))))
            }))
            .then(
                literal("value").then(
                    argument("value", IntegerArgumentType::with_min(0))
                        .executes(SetExecutor(CommandValueSet::Value)),
                ),
            )
            .then(
                literal("visible").then(
                    argument("visible", BoolArgumentType)
                        .executes(SetExecutor(CommandValueSet::Visible)),
                ),
            ),
    );

    dispatcher.register(
        command("bossbar", DESCRIPTION)
            .requires(PERMISSION)
            .then(add_node)
            .then(get_node)
            .then(literal("list").executes(ListExecutor))
            .then(remove_node)
            .then(set_node),
    );
}

#[cfg(test)]
mod golden;
