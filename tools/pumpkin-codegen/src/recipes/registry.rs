//! Reads the vanilla recipes and sorts them into token lists per family.

use super::RecipeTypes;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::Deserialize;
use std::{collections::BTreeMap, fs};

/// Reads every vanilla recipe JSON, keyed by its id.
pub(super) fn load_recipes() -> BTreeMap<String, RecipeTypes> {
    let dir = std::path::Path::new("../../assets/datapack/data/minecraft/recipe");
    let mut recipes_assets: BTreeMap<String, RecipeTypes> = BTreeMap::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("Missing recipe directory")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let key = format!("minecraft:{stem}");
        let content = fs::read_to_string(&path).expect("Failed to read recipe file");
        let recipe = serde_json::from_str::<RecipeTypes>(&content)
            .unwrap_or_else(|e| panic!("Failed to parse recipe {}: {e}", path.display()));
        recipes_assets.insert(key, recipe);
    }
    recipes_assets
}

/// The recipe token lists, one per recipe family.
pub(super) struct RecipeLists {
    pub(super) crafting_recipes: Vec<TokenStream>,
    pub(super) cooking_recipes: Vec<TokenStream>,
    pub(super) stonecutting_recipes: Vec<TokenStream>,
    pub(super) smithing_trim_recipes: Vec<TokenStream>,
    pub(super) smithing_transform_recipes: Vec<TokenStream>,
}

/// Sorts the recipes into their families as token streams.
pub(super) fn sort_recipes(recipes_assets: BTreeMap<String, RecipeTypes>) -> RecipeLists {
    let mut crafting_recipes = Vec::new();
    let mut cooking_recipes = Vec::new();
    let mut stonecutting_recipes = Vec::new();
    let mut smithing_trim_recipes = Vec::new();
    let mut smithing_transform_recipes = Vec::new();

    for (recipe_id, recipe) in recipes_assets {
        match recipe {
            RecipeTypes::Blasting(recipe) => {
                let mut common_cooking_token = TokenStream::new();
                recipe.to_tokens_with_id(&mut common_cooking_token, &recipe_id, 100);
                let blasting_token = quote! {
                    CookingRecipeType::Blasting (CookingRecipe {
                        #common_cooking_token
                    })
                };
                cooking_recipes.push(blasting_token);
            }
            RecipeTypes::CampfireCooking(recipe) => {
                let mut common_cooking_token = TokenStream::new();
                recipe.to_tokens_with_id(&mut common_cooking_token, &recipe_id, 100);
                let campfire_token = quote! {
                    CookingRecipeType::CampfireCooking (CookingRecipe {
                        #common_cooking_token
                    })
                };
                cooking_recipes.push(campfire_token);
            }
            RecipeTypes::CraftingShaped(recipe) => {
                crafting_recipes.push(recipe.to_token_stream());
            }
            RecipeTypes::CraftingShapeless(recipe) => {
                crafting_recipes.push(recipe.to_token_stream());
            }
            RecipeTypes::CraftingTransmute(recipe) => {
                crafting_recipes.push(recipe.to_token_stream());
            }
            RecipeTypes::CraftingDecoratedPot(recipe) => {
                crafting_recipes.push(recipe.to_token_stream());
            }
            RecipeTypes::Smelting(recipe) => {
                let mut common_cooking_token = TokenStream::new();
                recipe.to_tokens_with_id(&mut common_cooking_token, &recipe_id, 200);
                let smelting_token = quote! {
                    CookingRecipeType::Smelting(CookingRecipe {
                        #common_cooking_token
                    })
                };
                cooking_recipes.push(smelting_token);
            }
            RecipeTypes::SmithingTransform(recipe) => {
                smithing_transform_recipes.push(recipe.to_token_stream());
            }
            RecipeTypes::SmithingTrim(recipe) => {
                smithing_trim_recipes.push(recipe.to_token_stream());
            }
            RecipeTypes::Smoking(recipe) => {
                let mut common_cooking_token = TokenStream::new();
                recipe.to_tokens_with_id(&mut common_cooking_token, &recipe_id, 100);
                let smoking_token = quote! {
                    CookingRecipeType::Smoking(CookingRecipe{
                        #common_cooking_token
                    })
                };
                cooking_recipes.push(smoking_token);
            }
            RecipeTypes::Stonecutting(recipe) => {
                stonecutting_recipes.push(recipe.to_token_stream());
            }
            RecipeTypes::CraftingSpecial
            | RecipeTypes::CraftingSpecialBannerDuplicate
            | RecipeTypes::CraftingSpecialBookCloning
            | RecipeTypes::CraftingSpecialFireworkRocket
            | RecipeTypes::CraftingSpecialFireworkStar
            | RecipeTypes::CraftingSpecialFireworkStarFade
            | RecipeTypes::CraftingSpecialMapExtending
            | RecipeTypes::CraftingSpecialRepairItem
            | RecipeTypes::CraftingSpecialShieldDecoration
            | RecipeTypes::CraftingDye
            | RecipeTypes::CraftingImbue => {}
        }
    }
    RecipeLists {
        crafting_recipes,
        cooking_recipes,
        stonecutting_recipes,
        smithing_trim_recipes,
        smithing_transform_recipes,
    }
}
