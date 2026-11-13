//! The fixed part of the generated recipe file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_crafting_recipe_types_tokens() -> TokenStream {
    quote! {
        use crate::tag::Taggable;
        use crate::item::Item;
        use serde::{Serialize, Deserialize};

        #[derive(Clone, Debug, Serialize)]
        pub enum CraftingRecipeTypes {
            CraftingShaped {
                category: RecipeCategoryTypes,
                group: Option<&'static str>,
                show_notification: bool,
                key: &'static [(char, RecipeIngredientTypes)],
                pattern: &'static [&'static str],
                result: RecipeResultStruct,
            },
            CraftingShapeless {
                category: RecipeCategoryTypes,
                group: Option<&'static str>,
                ingredients: &'static [RecipeIngredientTypes],
                result: RecipeResultStruct,
            },
            CraftingTransmute {
                category: RecipeCategoryTypes,
                group: Option<&'static str>,
                input: RecipeIngredientTypes,
                material: RecipeIngredientTypes,
                result: RecipeResultStruct,
            },
            CraftingDecoratedPot {
                category: RecipeCategoryTypes,
            },
            CraftingSpecial,
        }

        #[allow(dead_code)]
        #[derive(Clone, Debug, Serialize)]
        pub struct CookingRecipe {
            /// Vanilla-compatible recipe ID (e.g., "minecraft:iron_ingot_from_smelting_iron_ore")
            pub recipe_id: &'static str,
            pub category: RecipeCategoryTypes,
            pub group: Option<&'static str>,
            pub ingredient: RecipeIngredientTypes,
            pub cookingtime: i32,
            pub experience: f32,
            pub result: RecipeResultStruct,
        }

        #[derive(Clone, Debug, Serialize)]
        pub enum CookingRecipeType {
            Blasting(CookingRecipe),
            Smelting(CookingRecipe),
            Smoking(CookingRecipe),
            CampfireCooking(CookingRecipe),
        }
        #[derive(Clone, Debug, Serialize)]
        pub enum CookingRecipeKind {
            Blasting,
            Smelting,
            Smoking,
            CampfireCooking,
        }

        impl From<&CookingRecipeType> for CookingRecipeKind {
            fn from(recipe_type: &CookingRecipeType) -> Self {
                match recipe_type {
                    CookingRecipeType::Blasting(_) => Self::Blasting,
                    CookingRecipeType::Smelting(_) => Self::Smelting,
                    CookingRecipeType::Smoking(_) => Self::Smoking,
                    CookingRecipeType::CampfireCooking(_) => Self::CampfireCooking,
                }
            }
        }

        impl From<CookingRecipeType> for CookingRecipeKind {
            fn from(recipe_type: CookingRecipeType) -> Self {
                match recipe_type {
                    CookingRecipeType::Blasting(_) => Self::Blasting,
                    CookingRecipeType::Smelting(_) => Self::Smelting,
                    CookingRecipeType::Smoking(_) => Self::Smoking,
                    CookingRecipeType::CampfireCooking(_) => Self::CampfireCooking,
                }
            }
        }
    }
}

pub(super) fn cooking_recipe_kind_tokens() -> TokenStream {
    quote! {
        impl CookingRecipeKind {
            #[must_use]
            pub const fn to_type(self, recipe: CookingRecipe) -> CookingRecipeType {
                match self {
                    Self::Blasting => CookingRecipeType::Blasting(recipe),
                    Self::Smelting => CookingRecipeType::Smelting(recipe),
                    Self::Smoking => CookingRecipeType::Smoking(recipe),
                    Self::CampfireCooking => CookingRecipeType::CampfireCooking(recipe),
                }
            }
        }

        #[derive(Clone, Debug, Serialize)]
        pub struct StonecutterRecipe {
            pub group: Option<&'static str>,
            pub ingredient: RecipeIngredientTypes,
            pub result: RecipeResultStruct,
        }

        #[derive(Clone, Debug, Serialize)]
        pub struct RecipeResultStruct {
            pub id: &'static str,
            pub count: u8,
        }

        #[derive(Clone, Debug, Serialize)]
        pub enum RecipeIngredientTypes {
            Simple(&'static str),
            Tagged(&'static str),
            OneOf(&'static [&'static str]),
        }

        impl RecipeIngredientTypes {
            #[must_use]
            pub fn match_item(&self, item: &Item) -> bool {
                match self {
                    Self::Simple(ingredient) => {
                        let name = format!("minecraft:{}", item.registry_key);
                        name == *ingredient
                    }
                    Self::Tagged(tag) => item
                        .is_tagged_with(tag)
                        .expect("Crafting recipe used invalid tag"),
                    Self::OneOf(ingredients) => {
                        let name = format!("minecraft:{}", item.registry_key);
                        ingredients.contains(&name.as_str())
                    }
                }
            }
        }

        #[derive(Clone, Debug, Serialize)]
        pub enum RecipeCategoryTypes {
            Equipment,
            Building,
            Restone,
            Misc,
            Food,
            Blocks,
        }
    }
}

pub(super) fn smithing_trim_recipe_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Debug, Serialize)]
        pub struct SmithingTrimRecipe {
            pub template: RecipeIngredientTypes,
            pub base: RecipeIngredientTypes,
            pub addition: RecipeIngredientTypes,
            pub pattern: &'static str,
        }

        impl SmithingTrimRecipe {
            #[must_use]
            pub fn matches(&self, template: &Item, base: &Item, addition: &Item) -> bool {
                self.template.match_item(template)
                    && self.base.match_item(base)
                    && self.addition.match_item(addition)
            }
        }

        #[must_use]
        pub fn get_smithing_trim_recipe(
            template: &Item,
            base: &Item,
            addition: &Item,
        ) -> Option<&'static SmithingTrimRecipe> {
            RECIPES_SMITHING_TRIM
                .iter()
                .find(|recipe| recipe.matches(template, base, addition))
        }

        #[derive(Clone, Debug, Serialize)]
        pub struct SmithingTransformRecipe {
            pub template: RecipeIngredientTypes,
            pub base: RecipeIngredientTypes,
            pub addition: RecipeIngredientTypes,
            pub result: RecipeResultStruct,
        }

        impl SmithingTransformRecipe {
            #[must_use]
            pub fn matches(&self, template: &Item, base: &Item, addition: &Item) -> bool {
                self.template.match_item(template)
                    && self.base.match_item(base)
                    && self.addition.match_item(addition)
            }
        }

        #[must_use]
        pub fn get_smithing_transform_recipe(
            template: &Item,
            base: &Item,
            addition: &Item,
        ) -> Option<&'static SmithingTransformRecipe> {
            RECIPES_SMITHING_TRANSFORM
                .iter()
                .find(|recipe| recipe.matches(template, base, addition))
        }

        /// Returns the trim material registry key for a given item, if it is a valid trim material.
        #[must_use]
        pub fn get_trim_material_for_item(item: &Item) -> Option<&'static str> {
            match item.registry_key {
                "amethyst_shard" => Some("minecraft:amethyst"),
                "copper_ingot" => Some("minecraft:copper"),
                "diamond" => Some("minecraft:diamond"),
                "emerald" => Some("minecraft:emerald"),
                "gold_ingot" => Some("minecraft:gold"),
                "iron_ingot" => Some("minecraft:iron"),
                "lapis_lazuli" => Some("minecraft:lapis"),
                "netherite_ingot" => Some("minecraft:netherite"),
                "quartz" => Some("minecraft:quartz"),
                "redstone" => Some("minecraft:redstone"),
                "resin_brick" | "resin_clump" => Some("minecraft:resin"),
                _ => None,
            }
        }
    }
}

pub(super) fn get_cooking_recipe_with_ingredient_tokens() -> TokenStream {
    quote! {
        #[must_use]
        pub fn get_cooking_recipe_with_ingredient(ingredient: &Item, recipe_type: CookingRecipeKind) -> Option<&'static CookingRecipe> {
            RECIPES_COOKING
                .iter()
                .find_map(|recipe| match (recipe, &recipe_type) {
                    (CookingRecipeType::Blasting(cooking_recipe), CookingRecipeKind::Blasting)
                    | (CookingRecipeType::Smelting(cooking_recipe), CookingRecipeKind::Smelting)
                    | (CookingRecipeType::Smoking(cooking_recipe), CookingRecipeKind::Smoking)
                    | (
                        CookingRecipeType::CampfireCooking(cooking_recipe),
                        CookingRecipeKind::CampfireCooking,
                    ) => {
                        cooking_recipe.ingredient.match_item(ingredient).then_some(cooking_recipe)
                    }
                    _ => None,
                })
        }

        /// Get the experience value for a recipe by its recipe ID.
        /// Used for calculating XP when extracting from furnace.
        /// Recipe IDs are in vanilla format like `"minecraft:iron_ingot_from_smelting_iron_ore"`
        #[must_use]
        pub fn get_recipe_experience(recipe_id: &str) -> Option<f32> {
            RECIPES_COOKING.iter().find_map(|recipe| {
                let cooking_recipe = match recipe {
                    CookingRecipeType::Blasting(r)
                    | CookingRecipeType::Smelting(r)
                    | CookingRecipeType::Smoking(r)
                    | CookingRecipeType::CampfireCooking(r) => r,
                };
                (cooking_recipe.recipe_id == recipe_id).then_some(cooking_recipe.experience)
            })
        }
    }
}
