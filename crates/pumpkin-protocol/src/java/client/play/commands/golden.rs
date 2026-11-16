//! A record of `ProtoNode::write_to` (`tests/golden/proto_node_bytes.txt`; rewrite it on purpose
//! with `KVASIR_BLESS=1`), made before it was split.
//!
//! One line per node and protocol version: the node, then the bytes written (hex) or the error.
//! The versions straddle 1.19 (parser ids replace parser names) and 1.21.6 (the restricted flag).

use std::fmt::Write as _;

use pumpkin_util::identifier::Identifier;
use pumpkin_util::version::JavaMinecraftVersion;

use super::{ArgumentType, ProtoNode, ProtoNodeType, StringProtoArgBehavior, SuggestionProviders};
use crate::VarInt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const VERSIONS: [JavaMinecraftVersion; 5] = [
    JavaMinecraftVersion::V_1_18,
    JavaMinecraftVersion::V_1_19,
    JavaMinecraftVersion::V_1_21_5,
    JavaMinecraftVersion::V_1_21_6,
    JavaMinecraftVersion::V_26_3,
];

fn children(ids: &[i32]) -> Box<[VarInt]> {
    ids.iter().map(|id| VarInt(*id)).collect()
}

const fn literal(
    name: &'static str,
    is_executable: bool,
    redirect_target: Option<i32>,
    restricted: bool,
) -> ProtoNodeType<'static> {
    ProtoNodeType::Literal {
        name,
        is_executable,
        redirect_target,
        restricted,
    }
}

const fn argument(
    name: &'static str,
    is_executable: bool,
    redirect_target: Option<i32>,
    parser: ArgumentType,
    override_suggestion_type: Option<SuggestionProviders>,
    restricted: bool,
) -> ProtoNodeType<'static> {
    ProtoNodeType::Argument {
        name,
        is_executable,
        redirect_target,
        parser,
        override_suggestion_type,
        restricted,
    }
}

fn nodes() -> Vec<ProtoNode<'static>> {
    let node = |ids: &[i32], node_type| ProtoNode {
        children: children(ids),
        node_type,
    };
    let mut nodes = vec![
        node(&[], ProtoNodeType::Root),
        node(&[1, 2, 300], ProtoNodeType::Root),
        node(&[], literal("say", false, None, false)),
        node(&[4], literal("say", true, None, false)),
        node(&[], literal("tp", false, Some(7), false)),
        node(&[], literal("tp", false, Some(0), false)),
        node(&[], literal("op", false, None, true)),
        node(&[2, 3], literal("all", true, Some(200), true)),
        node(
            &[],
            argument("flag", false, None, ArgumentType::Bool, None, false),
        ),
        node(
            &[],
            argument("flag", true, None, ArgumentType::Bool, None, false),
        ),
    ];
    nodes.extend(parsed_argument_nodes());
    nodes.extend(suggested_argument_nodes());
    nodes
}

/// Argument nodes with parsers that write properties.
fn parsed_argument_nodes() -> Vec<ProtoNode<'static>> {
    let node = |ids: &[i32], node_type| ProtoNode {
        children: children(ids),
        node_type,
    };
    vec![
        node(
            &[],
            argument(
                "text",
                true,
                None,
                ArgumentType::String(StringProtoArgBehavior::GreedyPhrase),
                None,
                false,
            ),
        ),
        node(
            &[],
            argument(
                "word",
                false,
                None,
                ArgumentType::String(StringProtoArgBehavior::QuotablePhrase),
                None,
                false,
            ),
        ),
        node(
            &[9],
            argument(
                "count",
                true,
                None,
                ArgumentType::Integer {
                    min: Some(1),
                    max: Some(64),
                },
                None,
                false,
            ),
        ),
        node(
            &[],
            argument(
                "big",
                false,
                None,
                ArgumentType::Long {
                    min: None,
                    max: Some(i64::MAX),
                },
                None,
                false,
            ),
        ),
        node(
            &[],
            argument(
                "targets",
                true,
                Some(5),
                ArgumentType::Entity { flags: 2 },
                None,
                false,
            ),
        ),
    ]
}

/// Argument nodes with suggestion providers.
fn suggested_argument_nodes() -> Vec<ProtoNode<'static>> {
    let node = |ids: &[i32], node_type| ProtoNode {
        children: children(ids),
        node_type,
    };
    vec![
        node(
            &[],
            argument(
                "recipe",
                false,
                None,
                ArgumentType::ResourceKey {
                    identifier: Identifier::vanilla_static("recipe"),
                },
                Some(SuggestionProviders::AllRecipes),
                false,
            ),
        ),
        node(
            &[],
            argument(
                "sound",
                true,
                None,
                ArgumentType::Resource {
                    identifier: Identifier::vanilla_static("sound_event"),
                },
                Some(SuggestionProviders::AvailableSounds),
                true,
            ),
        ),
        node(
            &[1],
            argument(
                "entity",
                false,
                Some(3),
                ArgumentType::Resource {
                    identifier: Identifier::vanilla_static("entity_type"),
                },
                Some(SuggestionProviders::SummonableEntities),
                true,
            ),
        ),
        node(
            &[],
            argument(
                "name",
                false,
                None,
                ArgumentType::Time { min: 5 },
                Some(SuggestionProviders::AskServer),
                false,
            ),
        ),
        node(
            &[],
            argument("pos", true, None, ArgumentType::BlockPos, None, true),
        ),
    ]
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[test]
fn proto_nodes_write_the_same_bytes() -> TestResult {
    let mut now = String::new();
    for node in nodes() {
        writeln!(now, "{:?} children {:?}", node.node_type, node.children)?;
        for version in &VERSIONS {
            let mut bytes = Vec::new();
            let written = match node.write_to(&mut bytes, version) {
                Ok(()) => hex(&bytes),
                Err(error) => format!("error {error:?}"),
            };
            writeln!(now, "  {version}: {written}")?;
        }
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("proto_node_bytes.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        std::fs::write(&path, now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
