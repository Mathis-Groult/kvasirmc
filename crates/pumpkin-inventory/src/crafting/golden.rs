//! A record of what `match_crafting_recipe` answers for every vanilla crafting recipe, made before
//! `recipe_matches` was split into small functions (`tests/golden/crafting_match.txt`; rewrite it
//! on purpose with `KVASIR_BLESS=1`).
//!
//! For each recipe the grid is filled in the ways that matter (exactly, shifted, mirrored, with a
//! stray item, with a wrong item, with an item missing, in another order) with items the
//! recipe's own ingredients accept, and one line holds what the matcher returned for each. A few
//! dynamic recipes from a provider are added.

use std::collections::HashMap;
use std::fmt::Write;

use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::recipes::{CraftingRecipeTypes, RECIPES_CRAFTING, RecipeIngredientTypes};
use pumpkin_protocol::codec::recipe::{
    DynamicRecipe, OwnedCraftingRecipe, OwnedRecipeIngredient, OwnedRecipeResult,
};

use super::crafting_inventory::CraftingInventory;
use super::crafting_screen_handler::{RecipeResult, match_crafting_recipe};
use super::recipe_provider::RecipeProvider;

/// Slots to fill: the slot index and the item in it.
type Layout = Vec<(usize, &'static Item)>;

struct Items {
    all: Vec<&'static Item>,
    first: HashMap<String, &'static Item>,
    last: HashMap<String, &'static Item>,
}

impl Items {
    fn new() -> Self {
        Self {
            all: (0..5000_u16).filter_map(Item::from_id).collect(),
            first: HashMap::new(),
            last: HashMap::new(),
        }
    }

    /// The lowest-id and the highest-id item the ingredient accepts.
    fn representative(&mut self, ingredient: &RecipeIngredientTypes, last: bool) -> &'static Item {
        let key = format!("{ingredient:?}");
        if !self.first.contains_key(&key) {
            let mut matching = self.all.iter().filter(|item| ingredient.match_item(item));
            let first = matching.next().copied().unwrap_or(&Item::BARRIER);
            let last = matching.next_back().copied().unwrap_or(first);
            self.first.insert(key.clone(), first);
            self.last.insert(key.clone(), last);
        }
        if last {
            self.last[&key]
        } else {
            self.first[&key]
        }
    }
}

fn describe(result: Option<RecipeResult>) -> String {
    result.map_or_else(
        || "-".to_owned(),
        |r| {
            format!(
                "{}*{}{}",
                r.item_id,
                r.count,
                r.pot_decorations
                    .map_or_else(String::new, |d| format!("{d:?}"))
            )
        },
    )
}

fn run(width: u8, height: u8, layout: &Layout, provider: Option<&dyn RecipeProvider>) -> String {
    let inventory = CraftingInventory::new(width, height);
    {
        let mut items = inventory
            .items
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (slot, item) in layout {
            items[*slot] = ItemStack::new(1, item);
        }
    }
    describe(match_crafting_recipe(&inventory, provider))
}

fn free_slot(layout: &Layout, size: usize) -> Option<usize> {
    (0..size).find(|slot| layout.iter().all(|(s, _)| s != slot))
}

/// The layouts of a shaped recipe: where the pattern goes in a 3x3 grid.
fn shaped_layouts(
    items: &mut Items,
    key: &[(char, RecipeIngredientTypes)],
    pattern: &[&str],
    last: bool,
) -> Vec<(&'static str, u8, Layout)> {
    let rows = pattern.len();
    let columns = pattern.first().map_or(0, |row| row.len());
    let place =
        |items: &mut Items, offset_x: usize, offset_y: usize, mirrored: bool, size: usize| {
            let mut layout = Layout::new();
            for (y, row) in pattern.iter().enumerate() {
                for (x, c) in row.chars().enumerate() {
                    if c == ' ' {
                        continue;
                    }
                    let Some((_, ingredient)) = key.iter().find(|(k, _)| *k == c) else {
                        continue;
                    };
                    let column = if mirrored { columns - 1 - x } else { x };
                    layout.push((
                        (y + offset_y) * size + column + offset_x,
                        items.representative(ingredient, last),
                    ));
                }
            }
            layout
        };
    let mut out = vec![("exact", 3, place(items, 0, 0, false, 3))];
    if columns <= 3 && rows <= 3 {
        out.push(("shifted", 3, place(items, 3 - columns, 3 - rows, false, 3)));
        out.push(("mirrored", 3, place(items, 0, 0, true, 3)));
    }
    if columns <= 2 && rows <= 2 {
        out.push(("small", 2, place(items, 0, 0, false, 2)));
        out.push(("small_mirrored", 2, place(items, 0, 0, true, 2)));
    }
    out
}

fn variants_of(layout: &Layout, size: usize) -> Vec<(&'static str, Layout)> {
    let mut out = Vec::new();
    if let Some(free) = free_slot(layout, size) {
        let mut extra = layout.clone();
        extra.push((free, &Item::DIRT));
        out.push(("extra", extra));
    }
    if let Some(first) = layout.first() {
        let mut wrong = layout.clone();
        wrong[0] = (
            first.0,
            if first.1.id == Item::DIRT.id {
                &Item::STONE
            } else {
                &Item::DIRT
            },
        );
        out.push(("wrong", wrong));
        let mut missing = layout.clone();
        missing.pop();
        out.push(("missing", missing));
    }
    out
}

fn shaped_line(
    items: &mut Items,
    key: &[(char, RecipeIngredientTypes)],
    pattern: &[&str],
    out: &mut String,
) -> std::fmt::Result {
    for last in [false, true] {
        for (name, size, layout) in shaped_layouts(items, key, pattern, last) {
            write!(
                out,
                " {}{}={}",
                name,
                if last { "/last" } else { "" },
                run(size, size, &layout, None)
            )?;
            if name == "exact" {
                for (variant, layout) in variants_of(&layout, 9) {
                    write!(out, " {}={}", variant, run(3, 3, &layout, None))?;
                }
            }
        }
    }
    Ok(())
}

fn ingredients_layout(
    items: &mut Items,
    ingredients: &[&RecipeIngredientTypes],
    last: bool,
) -> Layout {
    ingredients
        .iter()
        .enumerate()
        .map(|(slot, ingredient)| (slot, items.representative(ingredient, last)))
        .collect()
}

fn shapeless_line(
    items: &mut Items,
    ingredients: &[RecipeIngredientTypes],
    out: &mut String,
) -> std::fmt::Result {
    let refs: Vec<&RecipeIngredientTypes> = ingredients.iter().collect();
    let reversed: Vec<&RecipeIngredientTypes> = ingredients.iter().rev().collect();
    for (name, list, last) in [
        ("exact", &refs, false),
        ("last", &refs, true),
        ("reversed", &reversed, false),
    ] {
        let layout = ingredients_layout(items, list, last);
        write!(out, " {}={}", name, run(3, 3, &layout, None))?;
        if name == "exact" {
            for (variant, layout) in variants_of(&layout, 9) {
                write!(out, " {}={}", variant, run(3, 3, &layout, None))?;
            }
            // The last ingredient replaced by a second copy of the first.
            if let (Some(first), true) = (layout.first(), layout.len() > 1) {
                let mut duplicate = layout.clone();
                if let Some(last) = duplicate.last_mut() {
                    last.1 = first.1;
                }
                write!(out, " duplicate={}", run(3, 3, &duplicate, None))?;
            }
        }
    }
    Ok(())
}

/// The line of one recipe: a header, then what the matcher returned in each layout; the recipe's
/// own result is written as `=`.
fn recipe_line(
    items: &mut Items,
    index: usize,
    recipe: &CraftingRecipeTypes,
) -> Result<String, std::fmt::Error> {
    let mut body = String::new();
    let (header, own) = match recipe {
        CraftingRecipeTypes::CraftingShaped {
            key,
            pattern,
            result,
            ..
        } => {
            shaped_line(items, key, pattern, &mut body)?;
            (
                format!("{index} shaped {}*{}:", result.id, result.count),
                format!("{}*{}", result.id, result.count),
            )
        }
        CraftingRecipeTypes::CraftingShapeless {
            ingredients,
            result,
            ..
        } => {
            shapeless_line(items, ingredients, &mut body)?;
            (
                format!("{index} shapeless {}*{}:", result.id, result.count),
                format!("{}*{}", result.id, result.count),
            )
        }
        CraftingRecipeTypes::CraftingTransmute {
            input,
            material,
            result,
            ..
        } => {
            shapeless_line(items, &[input.clone(), material.clone()], &mut body)?;
            (
                format!("{index} transmute {}*{}:", result.id, result.count),
                format!("{}*{}", result.id, result.count),
            )
        }
        CraftingRecipeTypes::CraftingDecoratedPot { .. } => {
            pot_line(&mut body)?;
            (
                format!("{index} decorated_pot:"),
                "minecraft:decorated_pot*1".to_owned(),
            )
        }
        CraftingRecipeTypes::CraftingSpecial => (format!("{index} special"), String::new()),
    };
    let body = if own.is_empty() {
        body
    } else {
        body.replace(&own, "=")
    };
    Ok(format!("{header}{body}\n"))
}

fn recipe_lines(out: &mut String) -> std::fmt::Result {
    let mut items = Items::new();
    for (index, recipe) in RECIPES_CRAFTING.iter().enumerate() {
        out.push_str(&recipe_line(&mut items, index, recipe)?);
    }
    Ok(())
}

fn pot_line(out: &mut String) -> std::fmt::Result {
    let slots = [1, 3, 5, 7];
    let sherds = [
        &Item::BRICK,
        &Item::ANGLER_POTTERY_SHERD,
        &Item::BRICK,
        &Item::SKULL_POTTERY_SHERD,
    ];
    let pot: Layout = slots.iter().copied().zip(sherds).collect();
    write!(out, " exact={}", run(3, 3, &pot, None))?;
    write!(out, " small={}", run(2, 2, &pot[..2].to_vec(), None))?;
    let mut wrong = pot.clone();
    wrong[2].1 = &Item::DIRT;
    write!(out, " wrong={}", run(3, 3, &wrong, None))?;
    let mut extra = pot.clone();
    extra.push((4, &Item::BRICK));
    write!(out, " extra={}", run(3, 3, &extra, None))?;
    let mut missing = pot;
    missing.pop();
    write!(out, " missing={}", run(3, 3, &missing, None))
}

struct Provider(Vec<DynamicRecipe>);

impl RecipeProvider for Provider {
    fn get_dynamic_recipes(&self) -> Vec<DynamicRecipe> {
        self.0.clone()
    }
}

fn simple(name: &str) -> OwnedRecipeIngredient {
    OwnedRecipeIngredient::Simple(name.to_owned())
}

fn dynamic_provider() -> Provider {
    let result = |id: &str, count: u8| OwnedRecipeResult {
        item_id: id.to_owned(),
        count,
    };
    let shaped = |pattern: &[&str], key: Vec<(char, OwnedRecipeIngredient)>, id: &str| {
        DynamicRecipe::Crafting(OwnedCraftingRecipe::Shaped {
            recipe_id: None,
            category: pumpkin_data::recipes::RecipeCategoryTypes::Misc,
            group: None,
            show_notification: false,
            key,
            pattern: pattern.iter().map(|row| (*row).to_owned()).collect(),
            result: result(id, 2),
        })
    };
    Provider(vec![
        shaped(
            &["ab", "ba"],
            vec![
                ('a', simple("minecraft:emerald")),
                ('b', simple("minecraft:coal")),
            ],
            "minecraft:diamond",
        ),
        shaped(
            &["aab", " b "],
            vec![
                ('a', simple("minecraft:emerald")),
                (
                    'b',
                    OwnedRecipeIngredient::Tagged("minecraft:logs".to_owned()),
                ),
            ],
            "minecraft:netherite_ingot",
        ),
        DynamicRecipe::Crafting(OwnedCraftingRecipe::Shapeless {
            recipe_id: None,
            category: pumpkin_data::recipes::RecipeCategoryTypes::Misc,
            group: None,
            ingredients: vec![
                simple("minecraft:emerald"),
                simple("minecraft:emerald"),
                OwnedRecipeIngredient::OneOf(vec![
                    "minecraft:coal".to_owned(),
                    "minecraft:charcoal".to_owned(),
                ]),
            ],
            result: result("minecraft:gold_ingot", 3),
        }),
    ])
}

fn dynamic_lines(out: &mut String) -> std::fmt::Result {
    let provider = dynamic_provider();
    let emerald = &Item::EMERALD;
    let coal = &Item::COAL;
    let log = &Item::OAK_LOG;
    let cases: Vec<(&str, u8, Layout)> = vec![
        (
            "2x2",
            3,
            vec![(0, emerald), (1, coal), (3, coal), (4, emerald)],
        ),
        (
            "2x2_in_2x2",
            2,
            vec![(0, emerald), (1, coal), (2, coal), (3, emerald)],
        ),
        (
            "2x2_shifted",
            3,
            vec![(4, emerald), (5, coal), (7, coal), (8, emerald)],
        ),
        (
            "2x2_wrong",
            3,
            vec![(0, emerald), (1, emerald), (3, coal), (4, emerald)],
        ),
        (
            "3x2",
            3,
            vec![(0, emerald), (1, emerald), (2, log), (4, log)],
        ),
        (
            "3x2_mirrored",
            3,
            vec![(2, emerald), (1, emerald), (0, log), (4, log)],
        ),
        ("shapeless", 3, vec![(0, emerald), (4, coal), (8, emerald)]),
        (
            "shapeless_charcoal",
            3,
            vec![(2, &Item::CHARCOAL), (3, emerald), (5, emerald)],
        ),
        (
            "shapeless_extra",
            3,
            vec![(0, emerald), (4, coal), (8, emerald), (7, &Item::DIRT)],
        ),
        (
            "shapeless_three_emeralds",
            3,
            vec![(0, emerald), (1, emerald), (2, emerald)],
        ),
        ("nothing", 3, vec![]),
    ];
    for (name, size, layout) in cases {
        writeln!(
            out,
            "dynamic {name}: with={} without={}",
            run(size, size, &layout, Some(&provider)),
            run(size, size, &layout, None)
        )?;
    }
    Ok(())
}

#[test]
fn crafting_matches_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let mut now = String::new();
    recipe_lines(&mut now)?;
    dynamic_lines(&mut now)?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("crafting_match.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
