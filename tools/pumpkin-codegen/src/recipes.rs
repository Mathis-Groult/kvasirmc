mod registry;
mod static_items;

use registry::{RecipeLists, load_recipes, sort_recipes};
use std::{collections::BTreeMap, fs};

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::Deserialize;

/// Deserialized recipe entry from `recipes.json`, tagged by the `"type"` field.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum RecipeTypes {
    /// Blast furnace recipe.
    #[serde(rename = "minecraft:blasting")]
    Blasting(CookingRecipeStruct),
    /// Campfire cooking recipe.
    #[serde(rename = "minecraft:campfire_cooking")]
    CampfireCooking(CookingRecipeStruct),
    /// Shaped crafting table recipe.
    #[serde(rename = "minecraft:crafting_shaped")]
    CraftingShaped(CraftingShapedRecipeStruct),
    /// Shapeless crafting table recipe.
    #[serde(rename = "minecraft:crafting_shapeless")]
    CraftingShapeless(CraftingShapelessRecipeStruct),
    /// Transmute crafting recipe (preserves NBT/components from one slot to another).
    #[serde(rename = "minecraft:crafting_transmute")]
    CraftingTransmute(CraftingTransmuteRecipeStruct),
    /// Decorated-pot crafting recipe.
    #[serde(rename = "minecraft:crafting_decorated_pot")]
    CraftingDecoratedPot(CraftingDecoratedPotStruct),
    /// Furnace smelting recipe.
    #[serde(rename = "minecraft:smelting")]
    Smelting(CookingRecipeStruct),
    /// Smithing table transform recipe.
    #[serde(rename = "minecraft:smithing_transform")]
    SmithingTransform(SmithingTransformRecipeStruct),
    /// Smithing table armor-trim recipe.
    #[serde(rename = "minecraft:smithing_trim")]
    SmithingTrim(SmithingTrimRecipeStruct),
    /// Smoker cooking recipe.
    #[serde(rename = "minecraft:smoking")]
    Smoking(CookingRecipeStruct),
    /// Stonecutter recipe.
    #[serde(rename = "minecraft:stonecutting")]
    Stonecutting(StonecuttingRecipeStruct),
    /// Special crafting recipe types.
    #[serde(rename = "minecraft:crafting_special_bannerduplicate")]
    CraftingSpecialBannerDuplicate,
    #[serde(rename = "minecraft:crafting_special_bookcloning")]
    CraftingSpecialBookCloning,
    #[serde(rename = "minecraft:crafting_special_firework_rocket")]
    CraftingSpecialFireworkRocket,
    #[serde(rename = "minecraft:crafting_special_firework_star")]
    CraftingSpecialFireworkStar,
    #[serde(rename = "minecraft:crafting_special_firework_star_fade")]
    CraftingSpecialFireworkStarFade,
    #[serde(rename = "minecraft:crafting_special_mapextending")]
    CraftingSpecialMapExtending,
    #[serde(rename = "minecraft:crafting_special_repairitem")]
    CraftingSpecialRepairItem,
    #[serde(rename = "minecraft:crafting_special_shielddecoration")]
    CraftingSpecialShieldDecoration,
    #[serde(rename = "minecraft:crafting_dye")]
    CraftingDye,
    #[serde(rename = "minecraft:crafting_imbue")]
    CraftingImbue,
    /// Any other special crafting recipe type.
    #[serde(other)]
    #[serde(rename = "minecraft:crafting_special_*")]
    CraftingSpecial,
}

/// Deserialized smithing table transform recipe.
#[derive(Deserialize)]
pub struct SmithingTransformRecipeStruct {
    template: RecipeIngredientTypes,
    base: RecipeIngredientTypes,
    addition: RecipeIngredientTypes,
    result: RecipeResultStruct,
}

impl ToTokens for SmithingTransformRecipeStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let template = self.template.to_token_stream();
        let base = self.base.to_token_stream();
        let addition = self.addition.to_token_stream();
        let result = self.result.to_token_stream();

        tokens.extend(quote! {
            SmithingTransformRecipe {
                template: #template,
                base: #base,
                addition: #addition,
                result: #result,
            }
        });
    }
}

/// Deserialized smithing table armor-trim recipe.
#[derive(Deserialize)]
pub struct SmithingTrimRecipeStruct {
    template: RecipeIngredientTypes,
    base: RecipeIngredientTypes,
    addition: RecipeIngredientTypes,
    pattern: String,
}

impl ToTokens for SmithingTrimRecipeStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let template = self.template.to_token_stream();
        let base = self.base.to_token_stream();
        let addition = self.addition.to_token_stream();
        let pattern = &self.pattern;

        tokens.extend(quote! {
            SmithingTrimRecipe {
                template: #template,
                base: #base,
                addition: #addition,
                pattern: #pattern,
            }
        });
    }
}

/// Deserialized stonecutter recipe.
#[derive(Deserialize)]
pub struct StonecuttingRecipeStruct {
    /// Optional recipe group used for advancement tracking.
    group: Option<String>,
    /// The single ingredient required by this recipe.
    ingredient: RecipeIngredientTypes,
    /// The item produced by this recipe.
    result: RecipeResultStruct,
}

impl ToTokens for StonecuttingRecipeStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let group = if let Some(group) = &self.group {
            quote! { Some(#group) }
        } else {
            quote! { None }
        };
        let ingredient = self.ingredient.to_token_stream();
        let result = self.result.to_token_stream();

        tokens.extend(quote! {
            StonecutterRecipe {
                group: #group,
                ingredient: #ingredient,
                result: #result,
            }
        });
    }
}

/// Deserialized cooking recipe (furnace, blast furnace, smoker, or campfire).
#[derive(Deserialize)]
pub struct CookingRecipeStruct {
    /// UI category for this recipe.
    category: Option<RecipeCategoryTypes>,
    /// Optional recipe group used for advancement tracking.
    group: Option<String>,
    /// The single ingredient required by this recipe.
    ingredient: RecipeIngredientTypes,
    /// Number of ticks required to cook this recipe.
    cookingtime: Option<i32>,
    /// Experience points awarded when the result is extracted.
    experience: f32,
    /// The item produced by this recipe.
    result: RecipeResultStruct,
}

impl CookingRecipeStruct {
    /// Generate a recipe ID based on result, ingredient, and cooking type
    /// Format: minecraft:{result}_from_{`cooking_type`}_{ingredient}
    fn generate_recipe_id(&self, cooking_type: &str) -> String {
        let result_id = self.result.id.as_deref().unwrap_or("air");
        let result_name = result_id.strip_prefix("minecraft:").unwrap_or(result_id);
        let ingredient_name = match &self.ingredient {
            RecipeIngredientTypes::Simple(s) => {
                if s.starts_with('#') {
                    // Tagged ingredient - strip # and replace : with _
                    s.strip_prefix('#').unwrap_or(s).replace(':', "_")
                } else {
                    s.strip_prefix("minecraft:").unwrap_or(s).to_string()
                }
            }
            RecipeIngredientTypes::OneOf(items) => {
                // Use first item for ID generation
                items
                    .first()
                    .map_or("unknown", |s| s.strip_prefix("minecraft:").unwrap_or(s))
                    .to_string()
            }
        };
        format!("minecraft:{result_name}_from_{cooking_type}_{ingredient_name}")
    }

    /// Emits the cooking-recipe struct fields including the provided `recipe_id`.
    ///
    /// # Arguments
    /// – `tokens` – the token stream to extend.
    /// – `recipe_id` – the pre-generated vanilla-format recipe ID string.
    fn to_tokens_with_id(
        &self,
        tokens: &mut TokenStream,
        recipe_id: &str,
        default_cookingtime: i32,
    ) {
        let category = match &self.category {
            Some(category) => category.to_token_stream(),
            None => RecipeCategoryTypes::Misc.to_token_stream(),
        };
        let group = if let Some(group) = &self.group {
            quote! { Some(#group) }
        } else {
            quote! { None }
        };
        let ingredient = self.ingredient.to_token_stream();
        let cookingtime = self
            .cookingtime
            .unwrap_or(default_cookingtime)
            .to_token_stream();
        let experience = self.experience.to_token_stream();
        let result = self.result.to_token_stream();

        tokens.extend(quote! {
                recipe_id: #recipe_id,
                category: #category,
                group: #group,
                ingredient: #ingredient,
                cookingtime: #cookingtime,
                experience: #experience,
                result: #result,
        });
    }
}

impl ToTokens for CookingRecipeStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let category = match &self.category {
            Some(category) => category.to_token_stream(),
            None => RecipeCategoryTypes::Misc.to_token_stream(),
        };
        let group = if let Some(group) = &self.group {
            quote! { Some(#group) }
        } else {
            quote! { None }
        };
        let ingredient = self.ingredient.to_token_stream();
        let cookingtime = self.cookingtime.unwrap_or(200).to_token_stream();
        let experience = self.experience.to_token_stream();
        let result = self.result.to_token_stream();

        tokens.extend(quote! {
            //CookingRecipeType::Blasting,CampfireCooking,Smelting,Smoking{
                category: #category,
                group: #group,
                ingredient: #ingredient,
                cookingtime: #cookingtime,
                experience: #experience,
                result: #result,
            //}
        });
    }
}

/// Deserialized shaped crafting recipe.
#[derive(Deserialize)]
pub struct CraftingShapedRecipeStruct {
    /// UI category for this recipe.
    category: Option<RecipeCategoryTypes>,
    /// Optional recipe group used for advancement tracking.
    group: Option<String>,
    /// Whether to show a toast notification when unlocked.
    show_notification: Option<bool>,
    /// Map from pattern key characters to their ingredient types.
    key: BTreeMap<String, RecipeIngredientTypes>,
    /// Row strings defining the crafting grid layout.
    pattern: Vec<String>,
    /// The item produced by this recipe.
    result: RecipeResultStruct,
}

impl ToTokens for CraftingShapedRecipeStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let category = match &self.category {
            Some(category) => category.to_token_stream(),
            None => RecipeCategoryTypes::Misc.to_token_stream(),
        };
        let group = if let Some(group) = &self.group {
            quote! { Some(#group) }
        } else {
            quote! { None }
        };
        let show_notification = self.show_notification.unwrap_or(true);
        let key = self
            .key
            .iter()
            .map(|(key, ingredient)| {
                let key = key.chars().next().unwrap();
                quote! { (#key, #ingredient) }
            })
            .collect::<Vec<_>>();
        let pattern = self
            .pattern
            .iter()
            .map(quote::ToTokens::to_token_stream)
            .collect::<Vec<_>>();
        let result = self.result.to_token_stream();

        tokens.extend(quote! {
            CraftingRecipeTypes::CraftingShaped {
                category: #category,
                group: #group,
                show_notification: #show_notification,
                key: &[#(#key),*],
                pattern: &[#(#pattern),*],
                result: #result,
            }
        });
    }
}

/// Deserialized shapeless crafting recipe.
#[derive(Deserialize)]
pub struct CraftingShapelessRecipeStruct {
    /// UI category for this recipe.
    category: Option<RecipeCategoryTypes>,
    /// Optional recipe group used for advancement tracking.
    group: Option<String>,
    /// The unordered list of ingredients required.
    ingredients: Vec<RecipeIngredientTypes>,
    /// The item produced by this recipe.
    result: RecipeResultStruct,
}

impl ToTokens for CraftingShapelessRecipeStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let category = match &self.category {
            Some(category) => category.to_token_stream(),
            None => RecipeCategoryTypes::Misc.to_token_stream(),
        };
        let group = if let Some(group) = &self.group {
            quote! { Some(#group) }
        } else {
            quote! { None }
        };
        let ingredients = self
            .ingredients
            .iter()
            .map(quote::ToTokens::to_token_stream)
            .collect::<Vec<_>>();
        let result = self.result.to_token_stream();

        tokens.extend(quote! {
            CraftingRecipeTypes::CraftingShapeless {
                category: #category,
                group: #group,
                ingredients: &[#(#ingredients),*],
                result: #result,
            }
        });
    }
}

/// Deserialized transmute crafting recipe (copies components from input to result).
#[derive(Deserialize)]
pub struct CraftingTransmuteRecipeStruct {
    /// UI category for this recipe.
    category: Option<RecipeCategoryTypes>,
    /// Optional recipe group used for advancement tracking.
    group: Option<String>,
    /// The item whose data components are copied to the result.
    input: RecipeIngredientTypes,
    /// The material item consumed alongside `input`.
    material: RecipeIngredientTypes,
    /// The base item type of the result (inherits components from `input`).
    result: RecipeResultStruct,
}

impl ToTokens for CraftingTransmuteRecipeStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let category = match &self.category {
            Some(category) => category.to_token_stream(),
            None => RecipeCategoryTypes::Misc.to_token_stream(),
        };
        let group = if let Some(group) = &self.group {
            quote! { Some(#group) }
        } else {
            quote! { None }
        };
        let input = self.input.to_token_stream();
        let material = self.material.to_token_stream();
        let result = self.result.to_token_stream();

        tokens.extend(quote! {
            CraftingRecipeTypes::CraftingTransmute {
                category: #category,
                group: #group,
                input: #input,
                material: #material,
                result: #result,
            }
        });
    }
}

/// Deserialized decorated-pot crafting recipe.
#[derive(Deserialize)]
pub struct CraftingDecoratedPotStruct {
    /// UI category for this recipe.
    category: Option<RecipeCategoryTypes>,
}

impl ToTokens for CraftingDecoratedPotStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let category = match &self.category {
            Some(category) => category.to_token_stream(),
            None => RecipeCategoryTypes::Misc.to_token_stream(),
        };

        tokens.extend(quote! {
            CraftingRecipeTypes::CraftingDecoratedPot {
                category: #category,
            }
        });
    }
}

/// Deserialized recipe result specifying the output item and count.
#[derive(Deserialize)]
pub struct RecipeResultStruct {
    /// Registry key of the result item.
    id: Option<String>,
    /// Number of result items produced (defaults to 1).
    count: Option<u8>,
    // TODO: components: Option<RecipeResultComponentsStruct>,
}

impl ToTokens for RecipeResultStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let id = self.id.as_deref().unwrap_or("minecraft:air");
        let count = self.count.unwrap_or(1).to_token_stream();

        tokens.extend(quote! {
            RecipeResultStruct {
                id: #id,
                count: #count,
            }
        });
    }
}

/// Deserialized recipe ingredient, which is either a single item/tag or a list of alternatives.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum RecipeIngredientTypes {
    /// A single item registry key or tag (prefixed with `#`).
    Simple(String),
    /// A list of acceptable alternative item registry keys.
    OneOf(Vec<String>),
}

impl ToTokens for RecipeIngredientTypes {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = match self {
            Self::Simple(ingredient) => {
                if ingredient.starts_with('#') {
                    quote! { RecipeIngredientTypes::Tagged(#ingredient) }
                } else {
                    quote! { RecipeIngredientTypes::Simple(#ingredient) }
                }
            }
            Self::OneOf(ingredients) => {
                let ingredients = ingredients
                    .iter()
                    .map(quote::ToTokens::to_token_stream)
                    .collect::<Vec<_>>();
                quote! { RecipeIngredientTypes::OneOf(&[#(#ingredients),*]) }
            }
        };

        tokens.extend(name);
    }
}

/// Deserialized recipe UI category used to group recipes in the recipe book.
#[derive(Deserialize)]
pub enum RecipeCategoryTypes {
    /// Equipment recipes (tools, weapons, armor).
    #[serde(rename = "equipment")]
    Equipment,
    /// Building block recipes.
    #[serde(rename = "building")]
    Building,
    /// Redstone component recipes.
    #[serde(rename = "redstone")]
    Restone,
    /// Miscellaneous recipes that don't fit other categories.
    #[serde(rename = "misc")]
    Misc,
    /// Food item recipes.
    #[serde(rename = "food")]
    Food,
    /// Block recipes.
    #[serde(rename = "blocks")]
    Blocks,
}

impl ToTokens for RecipeCategoryTypes {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = match self {
            Self::Equipment => {
                quote! { RecipeCategoryTypes::Equipment }
            }
            Self::Building => {
                quote! { RecipeCategoryTypes::Building }
            }
            Self::Restone => {
                quote! { RecipeCategoryTypes::Restone }
            }
            Self::Misc => {
                quote! { RecipeCategoryTypes::Misc }
            }
            Self::Food => {
                quote! { RecipeCategoryTypes::Food }
            }
            Self::Blocks => {
                quote! { RecipeCategoryTypes::Blocks }
            }
        };

        tokens.extend(name);
    }
}

/// Reads recipe JSON files from the 26.2 datapack and emits the complete recipe constants and helpers `TokenStream`.
pub fn build() -> TokenStream {
    let recipes_assets = load_recipes();
    let RecipeLists {
        crafting_recipes,
        cooking_recipes,
        stonecutting_recipes,
        smithing_trim_recipes,
        smithing_transform_recipes,
    } = sort_recipes(recipes_assets);

    let imports_and_crafting_recipe_types_tokens =
        static_items::imports_and_crafting_recipe_types_tokens();
    let cooking_recipe_kind_tokens = static_items::cooking_recipe_kind_tokens();
    let smithing_trim_recipe_tokens = static_items::smithing_trim_recipe_tokens();
    let get_cooking_recipe_with_ingredient_tokens =
        static_items::get_cooking_recipe_with_ingredient_tokens();
    quote! {
        #imports_and_crafting_recipe_types_tokens

        #cooking_recipe_kind_tokens

        pub static RECIPES_CRAFTING: &[CraftingRecipeTypes] = &[
            #(#crafting_recipes),*
        ];
        pub static RECIPES_COOKING: &[CookingRecipeType] = &[
            #(#cooking_recipes ),*
        ];
        pub static RECIPES_STONECUTTING: &[StonecutterRecipe] = &[
            #(#stonecutting_recipes),*
        ];
        pub static RECIPES_SMITHING_TRIM: &[SmithingTrimRecipe] = &[
            #(#smithing_trim_recipes),*
        ];
        pub static RECIPES_SMITHING_TRANSFORM: &[SmithingTransformRecipe] = &[
            #(#smithing_transform_recipes),*
        ];

        #smithing_trim_recipe_tokens

        #get_cooking_recipe_with_ingredient_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_recipes() {
        let _ = build();
    }
}
