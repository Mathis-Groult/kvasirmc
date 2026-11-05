use crate::{
    argument_types::argument_type::{ArgumentType, JavaClientArgumentType},
    argument_types::block_predicate::parse_properties,
    context::command_context::CommandContext,
    errors::command_syntax_error::CommandSyntaxError,
    errors::error_types::CommandErrorType,
    string_reader::StringReader,
    suggestion::suggestions::{Suggestions, SuggestionsBuilder},
};
use std::{collections::HashMap, hash::BuildHasher};

use pumpkin_data::{Block, BlockStateId, translation};
use pumpkin_util::text::TextComponent;

pub const INVALID_BLOCK_ERROR_TYPE: CommandErrorType<1> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_ID_INVALID,
    translation::java::ARGUMENT_BLOCK_ID_INVALID,
);

/// `Block %s does not have property '%s'`.
pub const UNKNOWN_PROPERTY_ERROR_TYPE: CommandErrorType<2> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNKNOWN,
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNKNOWN,
);

/// `'%2$s' is not a valid value for property '%3$s' on block %1$s`.
pub const INVALID_PROPERTY_ERROR_TYPE: CommandErrorType<3> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_INVALID,
    translation::java::ARGUMENT_BLOCK_PROPERTY_INVALID,
);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BlockArgumentType;

impl<S: crate::source::CommandSource> ArgumentType<S> for BlockArgumentType {
    type Item = &'static Block;

    fn parse(&self, reader: &mut StringReader) -> Result<Self::Item, CommandSyntaxError> {
        let start = reader.cursor();
        while let Some(c) = reader.peek() {
            if c.is_alphanumeric() || c == '_' || c == ':' || c == '/' || c == '.' || c == '-' {
                reader.skip();
            } else {
                break;
            }
        }
        let block_name = &reader.string()[start..reader.cursor()];
        let normalized = if block_name.contains(':') {
            block_name.to_string()
        } else {
            format!("minecraft:{block_name}")
        };

        Block::from_name(&normalized)
            .ok_or_else(|| INVALID_BLOCK_ERROR_TYPE.create(reader, TextComponent::text(normalized)))
    }

    fn client_side_parser(&'_ self) -> JavaClientArgumentType {
        JavaClientArgumentType::BlockState
    }

    fn list_suggestions(
        &self,
        _context: &CommandContext<S>,
        builder: SuggestionsBuilder,
    ) -> Suggestions {
        builder.build()
    }
}

impl BlockArgumentType {
    pub fn get<S: crate::source::CommandSource>(
        context: &CommandContext<S>,
        name: &str,
    ) -> Result<&'static Block, CommandSyntaxError> {
        context.get_argument::<&'static Block>(name).copied()
    }
}

/// A block with the state a command asked for: `observer[facing=east]`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BlockStateSpec {
    pub block: &'static Block,
    pub state_id: BlockStateId,
}

/// A block id with optional `[property=value,...]`, as `setblock` and `fill` take it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BlockStateArgumentType;

/// Why a state could not be chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// The block has no property of this name.
    UnknownProperty(String),
    /// The block has the property, but never with this value.
    InvalidValue { property: String, value: String },
}

/// One state of a block: its id and its properties as name and value text.
type StateProperties = (BlockStateId, Vec<(String, String)>);

/// Picks the state that has every requested property. What the command does not name keeps the
/// value it has in the block's default state, as in vanilla.
///
/// # Errors
/// A requested property or value that no state of the block has.
fn choose_state<S: BuildHasher>(
    states: &[StateProperties],
    default: BlockStateId,
    requested: &HashMap<String, String, S>,
) -> Result<BlockStateId, StateError> {
    let default_properties = states
        .iter()
        .find(|(id, _)| *id == default)
        .map(|(_, properties)| properties.as_slice())
        .unwrap_or_default();
    // sorted so that the first error reported does not depend on the hash order
    let mut asked: Vec<(&String, &String)> = requested.iter().collect();
    asked.sort();
    for (property, value) in &asked {
        if !default_properties.iter().any(|(name, _)| name == *property) {
            return Err(StateError::UnknownProperty((*property).clone()));
        }
        let some_state_has_it = states.iter().any(|(_, properties)| {
            properties
                .iter()
                .any(|(name, found)| name == *property && found == *value)
        });
        if !some_state_has_it {
            return Err(StateError::InvalidValue {
                property: (*property).clone(),
                value: (*value).clone(),
            });
        }
    }
    states
        .iter()
        .find(|(_, properties)| {
            properties.iter().all(|(name, value)| {
                requested.get(name).map_or_else(
                    || {
                        default_properties
                            .iter()
                            .any(|(default_name, default_value)| {
                                default_name == name && default_value == value
                            })
                    },
                    |wanted| wanted == value,
                )
            })
        })
        .map(|(id, _)| *id)
        .ok_or_else(|| StateError::InvalidValue {
            // every value exists on its own but not together with the rest of the default state
            property: asked.first().map(|(p, _)| (*p).clone()).unwrap_or_default(),
            value: asked.first().map(|(_, v)| (*v).clone()).unwrap_or_default(),
        })
}

/// Every state of `block` with its properties as text.
fn states_of(block: &'static Block) -> Vec<StateProperties> {
    block
        .states
        .iter()
        .map(|state| {
            let properties = block
                .properties(state.id)
                .map(|properties| {
                    properties
                        .to_props()
                        .iter()
                        .map(|p| (p.key().to_str().to_owned(), p.value_str().to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            (state.id, properties)
        })
        .collect()
}

/// The state of `block` with these properties.
///
/// # Errors
/// See [`choose_state`].
pub fn state_with<S: BuildHasher>(
    block: &'static Block,
    requested: &HashMap<String, String, S>,
) -> Result<BlockStateId, StateError> {
    if requested.is_empty() {
        return Ok(block.default_state.id);
    }
    choose_state(&states_of(block), block.default_state.id, requested)
}

impl<S: crate::source::CommandSource> ArgumentType<S> for BlockStateArgumentType {
    type Item = BlockStateSpec;

    fn parse(&self, reader: &mut StringReader) -> Result<Self::Item, CommandSyntaxError> {
        let block = ArgumentType::<S>::parse(&BlockArgumentType, reader)?;
        let requested = parse_properties(reader)?;
        let state_id = state_with(block, &requested).map_err(|error| match error {
            StateError::UnknownProperty(property) => UNKNOWN_PROPERTY_ERROR_TYPE.create(
                reader,
                TextComponent::text(block.name),
                TextComponent::text(property),
            ),
            StateError::InvalidValue { property, value } => INVALID_PROPERTY_ERROR_TYPE.create(
                reader,
                TextComponent::text(block.name),
                TextComponent::text(value),
                TextComponent::text(property),
            ),
        })?;
        Ok(BlockStateSpec { block, state_id })
    }

    fn client_side_parser(&'_ self) -> JavaClientArgumentType {
        JavaClientArgumentType::BlockState
    }

    fn list_suggestions(
        &self,
        _context: &CommandContext<S>,
        builder: SuggestionsBuilder,
    ) -> Suggestions {
        builder.build()
    }
}

impl BlockStateArgumentType {
    pub fn get<S: crate::source::CommandSource>(
        context: &CommandContext<S>,
        name: &str,
    ) -> Result<BlockStateSpec, CommandSyntaxError> {
        context.get_argument::<BlockStateSpec>(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u16) -> BlockStateId {
        BlockStateId::new(n).expect("a real state id")
    }

    fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn asked(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        props(pairs).into_iter().collect()
    }

    /// A made-up block with two properties, `facing` (n, e) and `lit` (false, true).
    fn lamp() -> Vec<StateProperties> {
        vec![
            (id(1), props(&[("facing", "n"), ("lit", "false")])),
            (id(2), props(&[("facing", "n"), ("lit", "true")])),
            (id(3), props(&[("facing", "e"), ("lit", "false")])),
            (id(4), props(&[("facing", "e"), ("lit", "true")])),
        ]
    }

    #[test]
    fn what_is_not_named_keeps_its_value_from_the_default_state() {
        // default is facing=n, lit=true (state 2)
        assert_eq!(
            choose_state(&lamp(), id(2), &asked(&[("facing", "e")])),
            Ok(id(4))
        );
        assert_eq!(
            choose_state(&lamp(), id(2), &asked(&[("lit", "false")])),
            Ok(id(1))
        );
        assert_eq!(
            choose_state(&lamp(), id(2), &asked(&[("facing", "e"), ("lit", "false")])),
            Ok(id(3))
        );
    }

    #[test]
    fn a_property_the_block_lacks_or_a_value_it_never_has_is_refused() {
        assert_eq!(
            choose_state(&lamp(), id(1), &asked(&[("colour", "red")])),
            Err(StateError::UnknownProperty("colour".into()))
        );
        assert_eq!(
            choose_state(&lamp(), id(1), &asked(&[("facing", "up")])),
            Err(StateError::InvalidValue {
                property: "facing".into(),
                value: "up".into()
            })
        );
    }

    #[test]
    fn the_error_names_the_value_that_is_wrong_not_the_first_one_asked() {
        assert_eq!(
            choose_state(&lamp(), id(1), &asked(&[("facing", "e"), ("lit", "maybe")])),
            Err(StateError::InvalidValue {
                property: "lit".into(),
                value: "maybe".into()
            })
        );
    }

    #[test]
    fn a_block_without_properties_refuses_any() {
        let stone = vec![(id(1), Vec::new())];
        assert_eq!(
            choose_state(&stone, id(1), &asked(&[("facing", "n")])),
            Err(StateError::UnknownProperty("facing".into()))
        );
    }

    #[test]
    fn an_observer_can_face_east_and_keeps_its_other_properties() {
        let east = state_with(&Block::OBSERVER, &asked(&[("facing", "east")])).unwrap();
        let found = states_of(&Block::OBSERVER)
            .into_iter()
            .find(|(state, _)| *state == east)
            .unwrap()
            .1;
        assert!(
            found.contains(&("facing".to_owned(), "east".to_owned())),
            "{found:?}"
        );
        assert!(
            found.contains(&("powered".to_owned(), "false".to_owned())),
            "{found:?}"
        );
        assert_ne!(east, Block::OBSERVER.default_state.id);
        let up = state_with(&Block::OBSERVER, &asked(&[("facing", "up")])).unwrap();
        assert_ne!(up, east);
    }

    #[test]
    fn a_repeater_takes_its_delay() {
        let slow = state_with(&Block::REPEATER, &asked(&[("delay", "4")])).unwrap();
        let found = states_of(&Block::REPEATER)
            .into_iter()
            .find(|(state, _)| *state == slow)
            .unwrap()
            .1;
        assert!(found.contains(&("delay".to_owned(), "4".to_owned())));
        assert!(state_with(&Block::REPEATER, &asked(&[("delay", "5")])).is_err());
    }

    #[test]
    fn no_properties_gives_the_default_state() {
        assert_eq!(
            state_with(&Block::STONE, &HashMap::new()),
            Ok(Block::STONE.default_state.id)
        );
    }

    fn parse(text: &str) -> Result<BlockStateSpec, CommandSyntaxError> {
        let mut reader = StringReader::new(text);
        ArgumentType::<crate::source::DummySource>::parse(&BlockStateArgumentType, &mut reader)
    }

    #[test]
    fn the_argument_reads_an_id_with_or_without_brackets() {
        let plain = parse("minecraft:observer").unwrap();
        assert_eq!(plain.state_id, Block::OBSERVER.default_state.id);
        let faced = parse("observer[facing=west]").unwrap();
        assert_eq!(faced.block.id, Block::OBSERVER.id);
        assert_ne!(faced.state_id, plain.state_id);
        assert!(parse("observer[facing=sideways]").is_err());
        assert!(parse("observer[colour=red]").is_err());
        assert!(parse("observer[facing=west").is_err(), "an open bracket");
        assert!(parse("nonsense[facing=west]").is_err());
    }
}
