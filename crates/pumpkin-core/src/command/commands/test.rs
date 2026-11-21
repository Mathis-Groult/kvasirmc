use std::sync::Arc;

use pumpkin_data::translation::java;
use pumpkin_gametest::{GameTestBatchReport, GameTestReporter, GameTestRetryOptions};
use pumpkin_protocol::java::client::play::SuggestionProviders;
use pumpkin_util::PermissionLvl;
use pumpkin_util::identifier::Identifier;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;
use tracing::info;

use crate::command::CommandSender;
use crate::command::CommandSource;
use crate::command::argument_builder::{ArgumentBuilder, argument, command, literal};
use crate::command::argument_types::argument_type::{ArgumentType, JavaClientArgumentType};
use crate::command::argument_types::core::bool::BoolArgumentType;
use crate::command::argument_types::core::integer::IntegerArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::errors::command_syntax_error::CommandSyntaxError;
use crate::command::errors::error_types::CommandErrorType;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use crate::command::string_reader::StringReader;
use crate::command::suggestion::suggestions::{Suggestions, SuggestionsBuilder};
use crate::server::server_test_manager::{GameTestQueueEntry, enqueue_game_test, stop_game_tests};
use crate::world::World;

const DESCRIPTION: &str = "Runs a GameTest test instance.";
const PERMISSION: &str = "minecraft:command.test";

const ARG_TESTS: &str = "tests";
const ARG_NUMBER_OF_TIMES: &str = "numberOfTimes";
const ARG_UNTIL_FAILED: &str = "untilFailed";
const ARG_ROTATION_STEPS: &str = "rotationSteps";
const ARG_TESTS_PER_ROW: &str = "testsPerRow";

const TEST_POS_Z_OFFSET_FROM_PLAYER: i32 = 3;
const TEST_GRID_SPACING: i32 = 64;
const DEFAULT_TESTS_PER_ROW: i32 = 8;

const TEST_INSTANCE_REGISTRY: Identifier = Identifier::parse_static("minecraft:test_instance");

const TEST_NOT_FOUND_ERROR: CommandErrorType<2> = CommandErrorType::new(
    java::ARGUMENT_RESOURCE_SELECTOR_NOT_FOUND,
    java::ARGUMENT_RESOURCE_SELECTOR_NOT_FOUND,
);

struct CommandTestReporter {
    sender: CommandSender,
}

impl GameTestReporter for CommandTestReporter {
    fn send_message(&self, message: TextComponent) {
        self.sender.send_message(message);
    }
}

/// `GameTest` selectors are resource-location-like, but may also contain `*` and `?`.
/// Keep the Java client parser as `ResourceLocation` and ask the server for completions,
/// matching the behavior of the old command implementation.
struct TestInstanceArgumentType;

impl ArgumentType<CommandSource> for TestInstanceArgumentType {
    type Item = String;

    fn parse(&self, reader: &mut StringReader) -> Result<Self::Item, CommandSyntaxError> {
        let start = reader.cursor();
        while let Some(character) = reader.peek() {
            if is_allowed_in_test_selector(character) {
                reader.skip();
            } else {
                break;
            }
        }

        Ok(reader.string()[start..reader.cursor()].to_string())
    }

    fn list_suggestions(
        &self,
        context: &CommandContext,
        mut builder: SuggestionsBuilder,
    ) -> Suggestions {
        let current = builder.remaining().to_string();

        for name in context.server().datapack_manager.get_test_instance_names() {
            if resource_suggestion_matches(&current, &name) {
                builder = builder.suggest(name);
            }
        }

        builder.build()
    }

    fn client_side_parser(&self) -> JavaClientArgumentType {
        JavaClientArgumentType::ResourceLocation
    }

    fn override_suggestion_providers(&self) -> Option<SuggestionProviders> {
        Some(SuggestionProviders::AskServer)
    }

    fn examples(&self) -> Vec<String> {
        vec![
            "minecraft:test".to_string(),
            "minecraft:*".to_string(),
            "*".to_string(),
        ]
    }
}

impl TestInstanceArgumentType {
    fn get<'a>(context: &'a CommandContext, name: &str) -> Result<&'a str, CommandSyntaxError> {
        Ok(context.get_argument::<String>(name)?.as_str())
    }
}

/// The optional arguments of `/test run`, with vanilla's defaults.
struct RunOptions {
    number_of_times: i32,
    until_failed: bool,
    rotation_steps: i32,
    tests_per_row: i32,
}

impl RunOptions {
    fn read(context: &CommandContext) -> Result<Self, CommandSyntaxError> {
        let number_was_supplied = context.arguments.contains_key(ARG_NUMBER_OF_TIMES);
        let number_of_times = if number_was_supplied {
            IntegerArgumentType::get(context, ARG_NUMBER_OF_TIMES)?
        } else {
            1
        };

        let until_failed = if context.arguments.contains_key(ARG_UNTIL_FAILED) {
            BoolArgumentType::get(context, ARG_UNTIL_FAILED)?
        } else {
            // Vanilla RetryOptions.noRetries() is (1, true), while specifying
            // numberOfTimes without untilFailed defaults haltOnFailure to false.
            !number_was_supplied
        };

        let rotation_steps = if context.arguments.contains_key(ARG_ROTATION_STEPS) {
            IntegerArgumentType::get(context, ARG_ROTATION_STEPS)?
        } else {
            0
        };

        let tests_per_row = if context.arguments.contains_key(ARG_TESTS_PER_ROW) {
            IntegerArgumentType::get(context, ARG_TESTS_PER_ROW)?
        } else {
            DEFAULT_TESTS_PER_ROW
        }
        .max(1);
        Ok(Self {
            number_of_times,
            until_failed,
            rotation_steps,
            tests_per_row,
        })
    }

    const fn retry_options(&self) -> GameTestRetryOptions {
        GameTestRetryOptions::new(self.number_of_times, self.until_failed)
    }
}

/// Queues one request per selected test on a grid of `tests_per_row` columns.
fn queue_tests(
    options: &RunOptions,
    world: &Arc<World>,
    base_x: i32,
    base_z: i32,
    selected: Vec<String>,
    report: &Arc<GameTestBatchReport>,
) {
    for (index, test_id) in selected.into_iter().enumerate() {
        let index = index as i32;
        let column = index % options.tests_per_row;
        let row = index / options.tests_per_row;
        let test_x = base_x + column * TEST_GRID_SPACING;
        let test_z = base_z + row * TEST_GRID_SPACING;

        enqueue_game_test(GameTestQueueEntry::new(
            test_id.clone(),
            world.clone(),
            test_x,
            test_z,
            options.rotation_steps,
            options.retry_options(),
            report.clone(),
        ));

        info!(
            target: "pumpkin::gametest",
            test = %test_id,
            test_x,
            test_z,
            number_of_times = options.number_of_times,
            until_failed = options.until_failed,
            rotation_steps = options.rotation_steps,
            "Queued GameTest request"
        );
    }
}

struct RunExecutor;

impl CommandExecutor for RunExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let selector = TestInstanceArgumentType::get(context, ARG_TESTS)?;
        let selected: Vec<_> = context
            .server()
            .datapack_manager
            .get_test_instance_names()
            .into_iter()
            .filter(|name| resource_selector_matches(selector, name))
            .collect();

        if selected.is_empty() {
            return Err(TEST_NOT_FOUND_ERROR.create_without_context(
                TextComponent::text(selector.to_string()),
                TextComponent::text(TEST_INSTANCE_REGISTRY.to_string()),
            ));
        }

        // Vanilla TestCommand::run always clears the current GameTestTicker first.
        // Without this, issuing another /test run with a different retry mode lets
        // two runners own the same controller/structure concurrently.
        stop_game_tests();

        let options = RunOptions::read(context)?;

        let world = context.world().clone();
        let base_x = context.source.position.x.floor() as i32;
        let base_z = context.source.position.z.floor() as i32 + TEST_POS_Z_OFFSET_FROM_PLAYER;

        context
            .source
            .send_message(pumpkin_macros::translate_cross!(
                java::COMMANDS_TEST_RUN_RUNNING,
                java::COMMANDS_TEST_RUN_RUNNING,
                TextComponent::text(selected.len().to_string()),
            ));

        let report = Arc::new(GameTestBatchReport::new(
            Arc::new(CommandTestReporter {
                sender: context.source.output.clone(),
            }),
            selected.len(),
        ));

        queue_tests(&options, &world, base_x, base_z, selected, &report);

        Ok(1)
    }
}

struct StopExecutor;

impl CommandExecutor for StopExecutor {
    fn execute(&self, _context: &CommandContext) -> CommandExecutorResult {
        stop_game_tests();
        Ok(1)
    }
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    let tests_per_row =
        argument(ARG_TESTS_PER_ROW, IntegerArgumentType::any()).executes(RunExecutor);

    let rotation_steps = argument(ARG_ROTATION_STEPS, IntegerArgumentType::any())
        .executes(RunExecutor)
        .then(tests_per_row);

    let until_failed = argument(ARG_UNTIL_FAILED, BoolArgumentType)
        .executes(RunExecutor)
        .then(rotation_steps);

    let number_of_times = argument(ARG_NUMBER_OF_TIMES, IntegerArgumentType::with_min(0))
        .executes(RunExecutor)
        .then(until_failed);

    let tests = argument(ARG_TESTS, TestInstanceArgumentType)
        .executes(RunExecutor)
        .then(number_of_times);

    dispatcher.register(
        command("test", DESCRIPTION)
            .requires(PERMISSION)
            .then(literal("run").then(tests))
            .then(literal("stop").executes(StopExecutor)),
    );
}

const fn is_allowed_in_test_selector(character: char) -> bool {
    matches!(
        character,
        '0'..='9' | 'a'..='z' | '_' | '-' | '.' | '/' | ':' | '*' | '?'
    )
}

fn resource_suggestion_matches(input: &str, candidate: &str) -> bool {
    if input.is_empty() {
        return true;
    }

    let input = input.to_ascii_lowercase();
    let candidate = candidate.to_ascii_lowercase();

    if input.contains(':') {
        matches_suggestion_substring(&input, &candidate)
    } else if let Some((namespace, path)) = candidate.split_once(':') {
        matches_suggestion_substring(&input, namespace)
            || matches_suggestion_substring(&input, path)
    } else {
        matches_suggestion_substring(&input, &candidate)
    }
}

fn matches_suggestion_substring(pattern: &str, input: &str) -> bool {
    if input.starts_with(pattern) {
        return true;
    }

    input.char_indices().any(|(index, character)| {
        matches!(character, '.' | '_' | '/')
            && input[index + character.len_utf8()..].starts_with(pattern)
    })
}

fn resource_selector_matches(selector: &str, name: &str) -> bool {
    let selector = if selector.contains(':') {
        selector.to_string()
    } else {
        format!("minecraft:{selector}")
    };

    wildcard_match(selector.as_bytes(), name.as_bytes())
}

const fn wildcard_match(pattern: &[u8], value: &[u8]) -> bool {
    let (mut p, mut v) = (0usize, 0usize);
    let mut star = None;
    let mut retry_v = 0usize;

    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry_v = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            retry_v += 1;
            v = retry_v;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }

    p == pattern.len()
}

#[cfg(test)]
mod golden;
