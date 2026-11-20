//! The `/data` command tree: one `merge`, `get` and `remove` literal shared by every target
//! kind, and the `modify` subtree.

use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};

use crate::command::argument_builder::{
    ArgumentBuilder, LiteralArgumentBuilder, argument, command, literal,
};
use crate::command::argument_types::coordinates::block_pos::BlockPosArgumentType;
use crate::command::argument_types::core::double::DoubleArgumentType;
use crate::command::argument_types::core::integer::IntegerArgumentType;
use crate::command::argument_types::entity::EntityArgumentType;
use crate::command::argument_types::identifier::IdentifierArgumentType;
use crate::command::argument_types::nbt::{NbtCompoundArgumentType, NbtTagArgumentType};
use crate::command::argument_types::nbt_path::NbtPathArgumentType;
use crate::command::node::dispatcher::CommandDispatcher;

use super::{
    DESCRIPTION, GetExecutor, MergeExecutor, ModifyExecutor, ModifyMode, PERMISSION,
    RemoveExecutor, SourceMode, TargetKind,
};

/// A target kind and the literal that names it.
type TargetKinds = [(TargetKind, &'static str)];

fn make_target_arg(
    kind: TargetKind,
    name: &'static str,
) -> crate::command::argument_builder::RequiredArgumentBuilder {
    match kind {
        TargetKind::Entity => argument(name, EntityArgumentType::Entity),
        TargetKind::Block => argument(name, BlockPosArgumentType),
        TargetKind::Storage => argument(name, IdentifierArgumentType),
    }
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    let all_target_kinds = [
        (TargetKind::Entity, "entity"),
        (TargetKind::Block, "block"),
        (TargetKind::Storage, "storage"),
    ];

    let mut data_cmd = command("data", DESCRIPTION).requires(PERMISSION);

    let [merge_node, get_node, remove_node] = merge_get_remove_nodes(&all_target_kinds);
    data_cmd = data_cmd.then(merge_node).then(get_node).then(remove_node);

    data_cmd = data_cmd.then(modify_node(&all_target_kinds));

    dispatcher.register(data_cmd);
}

/// The `merge`, `get` and `remove` literals with one branch per target kind.
fn merge_get_remove_nodes(all_target_kinds: &TargetKinds) -> [LiteralArgumentBuilder; 3] {
    let mut merge_node = literal("merge");
    let mut get_node = literal("get");
    let mut remove_node = literal("remove");
    for &(target_kind, name) in all_target_kinds {
        // data merge <target> <nbt>
        merge_node = merge_node.then(literal(name).then(
            make_target_arg(target_kind, "target").then(
                argument("nbt", NbtCompoundArgumentType).executes(MergeExecutor { target_kind }),
            ),
        ));

        // data get <target> [<path>] [<scale>]
        get_node = get_node.then(
            literal(name).then(
                make_target_arg(target_kind, "target")
                    .executes(GetExecutor {
                        target_kind,
                        has_path: false,
                        has_scale: false,
                    })
                    .then(
                        argument("path", NbtPathArgumentType)
                            .executes(GetExecutor {
                                target_kind,
                                has_path: true,
                                has_scale: false,
                            })
                            .then(argument("scale", DoubleArgumentType::any()).executes(
                                GetExecutor {
                                    target_kind,
                                    has_path: true,
                                    has_scale: true,
                                },
                            )),
                    ),
            ),
        );

        // data remove <target> <path>
        remove_node = remove_node.then(literal(name).then(
            make_target_arg(target_kind, "target").then(
                argument("path", NbtPathArgumentType).executes(RemoveExecutor { target_kind }),
            ),
        ));
    }
    [merge_node, get_node, remove_node]
}

/// `modify <target kind> <target> <targetPath> <mode> ...` for every target kind and mode.
fn modify_node(all_target_kinds: &TargetKinds) -> LiteralArgumentBuilder {
    // data modify <target> <targetPath> (insert <index> | prepend | append | set | merge) ...
    let modify_modes = [
        ("insert", ModifyMode::Insert(0)),
        ("prepend", ModifyMode::Prepend),
        ("append", ModifyMode::Append),
        ("set", ModifyMode::Set),
        ("merge", ModifyMode::Merge),
    ];

    let mut modify_node = literal("modify");
    for &(target_kind, target_name) in all_target_kinds {
        let mut target_path_arg = argument("targetPath", NbtPathArgumentType);

        for &(mod_name, mod_mode) in &modify_modes {
            target_path_arg = target_path_arg.then(modify_mode_node(
                target_kind,
                mod_name,
                mod_mode,
                all_target_kinds,
            ));
        }

        modify_node = modify_node.then(
            literal(target_name).then(make_target_arg(target_kind, "target").then(target_path_arg)),
        );
    }
    modify_node
}

/// One modify mode (`insert <index>` or the mode's literal) with its `value`, `from` and
/// `string` sources. Each source kind adds its `from` and `string` literal again.
fn modify_mode_node(
    target_kind: TargetKind,
    mod_name: &'static str,
    mod_mode: ModifyMode,
    all_target_kinds: &TargetKinds,
) -> LiteralArgumentBuilder {
    // value <value>
    let mut sources = vec![
        literal("value").then(
            argument("value", NbtTagArgumentType).executes(ModifyExecutor {
                target_kind,
                modify_mode: mod_mode,
                source_kind: None,
                source_mode: SourceMode::Value,
            }),
        ),
    ];

    // `from` and `string` exist once, with one branch per source kind: a second `then` with
    // the same name replaces the first.
    let mut from_node = literal("from");
    let mut string_node = literal("string");
    for &(source_kind, source_name) in all_target_kinds {
        from_node = from_node.then(from_source(target_kind, mod_mode, source_kind, source_name));
        string_node = string_node.then(string_source(
            target_kind,
            mod_mode,
            source_kind,
            source_name,
        ));
    }
    sources.push(from_node);
    sources.push(string_node);

    // insert <index> (value | from | string), the other modes take them directly.
    if mod_name == "insert" {
        let mut index = argument("index", IntegerArgumentType::any());
        for source in sources {
            index = index.then(source);
        }
        literal("insert").then(index)
    } else {
        let mut mod_node = literal(mod_name);
        for source in sources {
            mod_node = mod_node.then(source);
        }
        mod_node
    }
}

/// from <source> [<sourcePath>]
fn from_source(
    target_kind: TargetKind,
    mod_mode: ModifyMode,
    source_kind: TargetKind,
    source_name: &'static str,
) -> LiteralArgumentBuilder {
    literal(source_name).then(
        make_target_arg(source_kind, "source")
            .executes(ModifyExecutor {
                target_kind,
                modify_mode: mod_mode,
                source_kind: Some(source_kind),
                source_mode: SourceMode::From { has_path: false },
            })
            .then(
                argument("sourcePath", NbtPathArgumentType).executes(ModifyExecutor {
                    target_kind,
                    modify_mode: mod_mode,
                    source_kind: Some(source_kind),
                    source_mode: SourceMode::From { has_path: true },
                }),
            ),
    )
}

/// string <source> [<sourcePath>] [<start>] [<end>]
fn string_source(
    target_kind: TargetKind,
    mod_mode: ModifyMode,
    source_kind: TargetKind,
    source_name: &'static str,
) -> LiteralArgumentBuilder {
    literal(source_name).then(
        make_target_arg(source_kind, "source")
            .executes(ModifyExecutor {
                target_kind,
                modify_mode: mod_mode,
                source_kind: Some(source_kind),
                source_mode: SourceMode::String {
                    has_path: false,
                    has_start: false,
                    has_end: false,
                },
            })
            .then(
                argument("sourcePath", NbtPathArgumentType)
                    .executes(ModifyExecutor {
                        target_kind,
                        modify_mode: mod_mode,
                        source_kind: Some(source_kind),
                        source_mode: SourceMode::String {
                            has_path: true,
                            has_start: false,
                            has_end: false,
                        },
                    })
                    .then(
                        argument("start", IntegerArgumentType::any())
                            .executes(ModifyExecutor {
                                target_kind,
                                modify_mode: mod_mode,
                                source_kind: Some(source_kind),
                                source_mode: SourceMode::String {
                                    has_path: true,
                                    has_start: true,
                                    has_end: false,
                                },
                            })
                            .then(argument("end", IntegerArgumentType::any()).executes(
                                ModifyExecutor {
                                    target_kind,
                                    modify_mode: mod_mode,
                                    source_kind: Some(source_kind),
                                    source_mode: SourceMode::String {
                                        has_path: true,
                                        has_start: true,
                                        has_end: true,
                                    },
                                },
                            )),
                    ),
            ),
    )
}
