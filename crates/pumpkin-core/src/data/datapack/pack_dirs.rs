//! The directories of one namespace that `load_pack_contents` reads, a kind of content at a time.

use std::path::Path;

use super::{
    PackContentAccumulators, PackContentCounts, advancement_loader, context_provider_loader,
    damage_type_loader, dynamic_registry_loader, function_loader, item_modifier_loader,
    load_recipes_from_dir, load_test_instances_from_dir, loot_table_loader, predicate_loader,
    slot_source_loader, structure_loader, tag_loader, trade_loader,
};

/// Recipes, functions, tags, test instances, damage types, context providers and loot tables of one namespace.
pub(super) fn load_gameplay_dirs(
    namespace: &str,
    ns_path: &Path,
    acc: &mut PackContentAccumulators<'_>,
    counts: &mut PackContentCounts,
) {
    // Load recipes
    for recipe_sub in ["recipe", "recipes"] {
        let recipe_dir = ns_path.join(recipe_sub);
        if recipe_dir.is_dir() {
            load_recipes_from_dir(namespace, &recipe_dir, acc.recipes, &mut counts.recipes);
        }
    }

    // Load functions
    for fn_sub in ["function", "functions"] {
        let fn_dir = ns_path.join(fn_sub);
        if fn_dir.is_dir() {
            let before = acc.functions.len();
            function_loader::load_functions_from_dir(namespace, &fn_dir, acc.functions);
            counts.functions += acc.functions.len() - before;
        }
    }

    // Load tags
    let tags_dir = ns_path.join("tags");
    if tags_dir.is_dir() {
        function_loader::load_function_tags_from_dir(namespace, &tags_dir, acc.function_tags);
        counts.tags += tag_loader::load_tags_from_dir(namespace, &tags_dir, acc.tags);
    }
    // Load game test instances
    let test_instance_dir = ns_path.join("test_instance");
    if test_instance_dir.is_dir() {
        counts.test_instances +=
            load_test_instances_from_dir(namespace, &test_instance_dir, acc.test_instances);
    }

    // Load damage types
    for dt_sub in ["damage_type", "damage_types"] {
        let dt_dir = ns_path.join(dt_sub);
        if dt_dir.is_dir() {
            counts.damage_types += damage_type_loader::load_damage_types_from_dir(
                namespace,
                &dt_dir,
                acc.damage_types,
            );
        }
    }

    // Load context int providers
    for int_sub in ["context_int_provider", "context_int_providers"] {
        let provider_dir = ns_path.join(int_sub);
        if provider_dir.is_dir() {
            counts.int_providers += context_provider_loader::load_context_providers_from_dir(
                namespace,
                &provider_dir,
                acc.context_int_providers,
            );
        }
    }

    // Load context float providers
    for float_sub in ["context_float_provider", "context_float_providers"] {
        let provider_dir = ns_path.join(float_sub);
        if provider_dir.is_dir() {
            counts.float_providers += context_provider_loader::load_context_providers_from_dir(
                namespace,
                &provider_dir,
                acc.context_float_providers,
            );
        }
    }

    // Load loot tables
    for lt_sub in ["loot_table", "loot_tables"] {
        let lt_dir = ns_path.join(lt_sub);
        if lt_dir.is_dir() {
            counts.loot_tables +=
                loot_table_loader::load_loot_tables_from_dir(namespace, &lt_dir, acc.loot_tables);
        }
    }
}

/// Advancements, predicates, item modifiers and slot sources of one namespace.
pub(super) fn load_logic_dirs(
    namespace: &str,
    ns_path: &Path,
    acc: &mut PackContentAccumulators<'_>,
    counts: &mut PackContentCounts,
) {
    // Load advancements
    for adv_sub in ["advancement", "advancements"] {
        let adv_dir = ns_path.join(adv_sub);
        if adv_dir.is_dir() {
            counts.advancements += advancement_loader::load_advancements_from_dir(
                namespace,
                &adv_dir,
                acc.advancements,
            );
        }
    }

    // Load predicates
    for pred_sub in ["predicate", "predicates"] {
        let pred_dir = ns_path.join(pred_sub);
        if pred_dir.is_dir() {
            counts.predicates +=
                predicate_loader::load_predicates_from_dir(namespace, &pred_dir, acc.predicates);
        }
    }

    // Load item modifiers
    for im_sub in ["item_modifier", "item_modifiers"] {
        let im_dir = ns_path.join(im_sub);
        if im_dir.is_dir() {
            counts.item_modifiers += item_modifier_loader::load_item_modifiers_from_dir(
                namespace,
                &im_dir,
                acc.item_modifiers,
            );
        }
    }

    // Load slot sources
    for ss_sub in ["slot_source", "slot_sources"] {
        let ss_dir = ns_path.join(ss_sub);
        if ss_dir.is_dir() {
            counts.slot_sources += slot_source_loader::load_slot_sources_from_dir(
                namespace,
                &ss_dir,
                acc.slot_sources,
            );
        }
    }
}

/// Structures, template pools, processor lists and dynamic registries of one namespace.
pub(super) fn load_world_dirs(
    namespace: &str,
    ns_path: &Path,
    acc: &mut PackContentAccumulators<'_>,
    counts: &mut PackContentCounts,
) {
    // Load structure templates
    for st_sub in ["structure", "structures"] {
        let st_dir = ns_path.join(st_sub);
        if st_dir.is_dir() {
            counts.structures += structure_loader::load_structures_from_dir(namespace, &st_dir);
        }
    }

    // Load worldgen template pools and processor lists
    let worldgen_dir = ns_path.join("worldgen");
    if worldgen_dir.is_dir() {
        let pool_dir = worldgen_dir.join("template_pool");
        if pool_dir.is_dir() {
            counts.template_pools +=
                structure_loader::load_template_pools_from_dir(namespace, &pool_dir);
        }
        let proc_dir = worldgen_dir.join("processor_list");
        if proc_dir.is_dir() {
            counts.processor_lists +=
                structure_loader::load_processor_lists_from_dir(namespace, &proc_dir);
        }
    }

    // Load dynamic registry entries (wolf_variant, cat_variant, frog_variant, instrument,
    // trim_material, trim_pattern, banner_pattern, decorated_pot_pattern, chat_type, etc.)
    counts.dynamic_registries += dynamic_registry_loader::load_dynamic_registries_from_ns(
        namespace,
        ns_path,
        acc.dynamic_registries,
    );
}

/// Trade sets, villager trades and their tags of one namespace.
pub(super) fn load_trade_dirs(
    namespace: &str,
    ns_path: &Path,
    acc: &mut PackContentAccumulators<'_>,
    counts: &mut PackContentCounts,
) {
    // Load trade sets
    for ts_sub in ["trade_set", "trade_sets"] {
        let ts_dir = ns_path.join(ts_sub);
        if ts_dir.is_dir() {
            counts.trade_sets += trade_loader::load_trade_sets_from_dir(
                namespace,
                &ts_dir,
                &mut acc.trade_registry.trade_sets,
            );
        }
    }

    // Load villager trades
    for vt_sub in ["villager_trade", "villager_trades"] {
        let vt_dir = ns_path.join(vt_sub);
        if vt_dir.is_dir() {
            counts.trades += trade_loader::load_trades_from_dir(
                namespace,
                &vt_dir,
                &mut acc.trade_registry.trades,
            );
        }
    }

    // Load villager trade tags
    let tags_dir = ns_path.join("tags");
    if tags_dir.is_dir() {
        for vt_tag_sub in ["villager_trade", "villager_trades"] {
            let vt_tag_dir = tags_dir.join(vt_tag_sub);
            if vt_tag_dir.is_dir() {
                counts.trade_tags += trade_loader::load_trade_tags_from_dir(
                    namespace,
                    &vt_tag_dir,
                    &mut acc.trade_registry.trade_tags,
                );
            }
        }
    }
}
