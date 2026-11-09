//! A record of the command tree that `default_dispatcher` builds, made before the long
//! registration functions were split (`tests/golden/command_tree.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Every node is described by what a client or the parser can tell apart: kind, name, whether it
//! runs a command, whether it forks, which redirect modifier it has, where it redirects, how many
//! requirements it has and and for arguments the client
//! parser and suggestion provider. The file has one line per top-level command (node count and a
//! hash of its whole subtree) followed by the full text of the `execute` subtree, the largest one.

use std::collections::BTreeMap;
use std::fmt::Write;

use pumpkin_config::CommandsConfig;
use pumpkin_util::permission::PermissionManager;

use super::default_dispatcher;
use crate::command::node::RedirectModifier;
use crate::command::node::attached::{AttachedNode, NodeId};
use crate::command::node::tree::ROOT_NODE_ID;

type Dispatcher = crate::command::dispatcher::CommandDispatcher;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The text that describes one node, without its children.
fn describe(dispatcher: &Dispatcher, node: &AttachedNode) -> String {
    let (kind, name, command, owned) = match node {
        AttachedNode::Root(n) => ("root", String::new(), n.owned.command.is_some(), &n.owned),
        AttachedNode::Literal(n) => (
            "literal",
            n.meta.literal.to_string(),
            n.owned.command.is_some(),
            &n.owned,
        ),
        AttachedNode::Command(n) => (
            "command",
            n.meta.literal.to_string(),
            n.owned.command.is_some(),
            &n.owned,
        ),
        AttachedNode::Argument(n) => (
            "argument",
            n.meta.name.to_string(),
            n.owned.command.is_some(),
            &n.owned,
        ),
    };
    let modifier = match owned.modifier {
        RedirectModifier::KeepSource => "keep",
        RedirectModifier::Custom(_) => "custom",
    };
    let redirect = node
        .redirect()
        .and_then(|redirection| dispatcher.tree.resolve(redirection))
        .map_or_else(|| "-".to_string(), |id| id.0.to_string());
    let mut text = format!(
        "{kind} {name} runs={command} forks={} modifier={modifier} redirect={redirect} requirements={}",
        owned.forks,
        owned.requirements.0.len(),
    );
    if let AttachedNode::Argument(argument) = node {
        let _ = write!(
            text,
            " parser={:?} suggestions={} override={:?}",
            argument.meta.argument_type.client_side_parser(),
            argument.meta.suggestion_provider.is_some(),
            argument.meta.argument_type.override_suggestion_providers(),
        );
    }
    text
}

/// The nodes below `id` (and `id` itself), depth first, children in name order.
fn subtree(dispatcher: &Dispatcher, id: NodeId, depth: usize, out: &mut Vec<String>) {
    let node = &dispatcher.tree[id];
    out.push(format!(
        "{}{}",
        "  ".repeat(depth),
        describe(dispatcher, node)
    ));
    let children: BTreeMap<&String, NodeId> =
        node.children_ref().iter().map(|(k, v)| (k, *v)).collect();
    for child in children.into_values() {
        subtree(dispatcher, child, depth + 1, out);
    }
}

fn record() -> Result<String, std::fmt::Error> {
    let dispatcher = default_dispatcher(&PermissionManager::new(), &CommandsConfig::default());
    let root = &dispatcher.tree[ROOT_NODE_ID];
    let top: BTreeMap<&String, NodeId> = root.children_ref().iter().map(|(k, v)| (k, *v)).collect();

    let mut out = String::new();
    let mut execute = Vec::new();
    for (name, id) in top {
        let mut lines = Vec::new();
        subtree(&dispatcher, id, 0, &mut lines);
        writeln!(
            out,
            "{name} nodes={} hash={:016x}",
            lines.len(),
            fnv(&lines.join("\n"))
        )?;
        if name == "execute" {
            execute = lines;
        }
    }
    writeln!(out, "---- execute")?;
    for line in execute {
        writeln!(out, "{line}")?;
    }
    Ok(out)
}

#[test]
fn command_tree_is_built_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let now = record()?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("command_tree.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
