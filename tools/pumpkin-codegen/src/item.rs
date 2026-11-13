mod registry;
mod static_items;

use registry::{
    JavaItemTokens, bedrock_item_constants, bedrock_item_definitions, bedrock_to_java_arms,
    java_item_tokens, java_to_bedrock_remaps, load_bedrock_item_components, mapping_constants,
};
mod basic;
mod combat;
mod consumption;
mod equipment;

use crate::block::{Block, BlockAssets};
use crate::enchantments::AttributeModifierSlot;
use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::{Span, TokenStream};
use pumpkin_nbt::{Nbt, compound::NbtCompound};
use pumpkin_util::registry::TagType;
use pumpkin_util::text::TextContent;
use pumpkin_util::{registry::RegistryEntryList, text::TextComponent};
use quote::{ToTokens, format_ident, quote};
use serde::Deserialize;
use serde_repr::Deserialize_repr;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Cursor,
};
use syn::{Ident, LitBool, LitByteStr, LitFloat, LitInt, LitStr};

/// Deserialized item entry from `items.json`.
#[derive(Deserialize)]
pub struct Item {
    /// Numeric protocol ID for this item.
    pub id: u16,
    /// All data components attached to this item.
    pub components: ItemComponents,
}

/// All deserialized data components for a single item as stored in `items.json`.
#[derive(Deserialize)]
pub struct ItemComponents {
    /// Display name translation component.
    #[serde(rename = "minecraft:item_name")]
    pub item_name: TextComponent,
    /// Maximum number of items per stack.
    #[serde(rename = "minecraft:max_stack_size")]
    pub max_stack_size: u8,
    /// Use cooldown component.
    #[serde(rename = "minecraft:use_cooldown")]
    pub use_cooldown: Option<UseCooldownComponent>,
    /// Jukebox song key if this item is a music disc, otherwise `None`.
    #[serde(rename = "minecraft:jukebox_playable")]
    pub jukebox_playable: Option<String>,
    /// Current damage value of a damageable item, if any.
    #[serde(rename = "minecraft:damage")]
    pub damage: Option<u16>,
    /// Maximum durability of a damageable item, if any.
    #[serde(rename = "minecraft:max_damage")]
    pub max_damage: Option<u16>,
    /// Attribute modifiers applied when the item is held or worn, if any.
    #[serde(rename = "minecraft:attribute_modifiers")]
    pub attribute_modifiers: Option<Vec<Modifier>>,
    /// Tool component containing mining rules, if this item is a tool.
    #[serde(rename = "minecraft:tool")]
    pub tool: Option<ToolComponent>,
    /// Food component, present if this item is edible.
    #[serde(rename = "minecraft:food")]
    pub food: Option<FoodComponent>,
    /// Equippable component, present if this item can be worn in an armor slot.
    #[serde(rename = "minecraft:equippable")]
    pub equippable: Option<EquippableComponent>,
    /// Consumable component, present if this item has a custom use animation or duration.
    #[serde(rename = "minecraft:consumable")]
    pub consumable: Option<Consumable>,
    /// Present if this item can block attacks (e.g., shields).
    #[serde(rename = "minecraft:blocks_attacks")]
    pub blocks_attacks: Option<BlocksAttacks>,
    /// Present if this item grants death protection (e.g., totem of undying).
    #[serde(rename = "minecraft:death_protection")]
    pub death_protection: Option<DeathProtection>,
    /// Damage type resistance, if this item is immune to specific damage types.
    #[serde(rename = "minecraft:damage_resistant")]
    pub damage_resistant: Option<DamageResistantComponent>,
    #[serde(rename = "minecraft:weapon")]
    pub weapon: Option<WeaponComponent>,
    #[serde(rename = "minecraft:enchantable")]
    pub enchantable: Option<EnchantableComponent>,
    #[serde(rename = "minecraft:attack_range")]
    pub attack_range: Option<AttackRangeComponent>,
    #[serde(rename = "minecraft:banner_patterns")]
    pub banner_patterns: Option<serde_json::Value>,
    #[serde(rename = "minecraft:bees")]
    pub bees: Option<serde_json::Value>,
    #[serde(rename = "minecraft:block_state")]
    pub block_state: Option<serde_json::Value>,
    #[serde(rename = "minecraft:block_transformer")]
    pub block_transformer: Option<String>,
    #[serde(rename = "minecraft:break_sound")]
    pub break_sound: Option<String>,
    #[serde(rename = "minecraft:brewing_fuel")]
    pub brewing_fuel: Option<serde_json::Value>,
    #[serde(rename = "minecraft:bucket_entity_data")]
    pub bucket_entity_data: Option<serde_json::Value>,
    #[serde(rename = "minecraft:bundle_contents")]
    pub bundle_contents: Option<serde_json::Value>,
    #[serde(rename = "minecraft:charged_projectiles")]
    pub charged_projectiles: Option<serde_json::Value>,
    #[serde(rename = "minecraft:chicken/variant")]
    pub chicken_variant: Option<serde_json::Value>,
    #[serde(rename = "minecraft:container")]
    pub container: Option<serde_json::Value>,
    #[serde(rename = "minecraft:compostable")]
    pub compostable: Option<CompostableComponent>,
    #[serde(rename = "minecraft:cooking_fuel")]
    pub cooking_fuel: Option<CookingFuelComponent>,
    #[serde(rename = "minecraft:cushion_color")]
    pub cushion_color: Option<String>,
    #[serde(rename = "minecraft:damage_type")]
    pub damage_type: Option<String>,
    #[serde(rename = "minecraft:debug_stick_state")]
    pub debug_stick_state: Option<serde_json::Value>,
    #[serde(rename = "minecraft:dye")]
    pub dye: Option<String>,
    #[serde(rename = "minecraft:enchantment_glint_override")]
    pub enchantment_glint_override: Option<bool>,
    #[serde(rename = "minecraft:enchantments")]
    pub enchantments: Option<serde_json::Value>,
    #[serde(rename = "minecraft:entity_data")]
    pub entity_data: Option<serde_json::Value>,
    #[serde(rename = "minecraft:fireworks")]
    pub fireworks: Option<serde_json::Value>,
    #[serde(rename = "minecraft:glider")]
    pub glider: Option<serde_json::Value>,
    #[serde(rename = "minecraft:instrument")]
    pub instrument: Option<String>,
    #[serde(rename = "minecraft:interact_animation")]
    pub interact_animation: Option<SwingAnimationComponent>,
    #[serde(rename = "minecraft:item_model")]
    pub item_model: Option<String>,
    #[serde(rename = "minecraft:kinetic_weapon")]
    pub kinetic_weapon: Option<KineticWeaponComponent>,
    #[serde(rename = "minecraft:lore")]
    pub lore: Option<serde_json::Value>,
    #[serde(rename = "minecraft:map_color")]
    pub map_color: Option<serde_json::Value>,
    #[serde(rename = "minecraft:map_decorations")]
    pub map_decorations: Option<serde_json::Value>,
    #[serde(rename = "minecraft:minimum_attack_charge")]
    pub minimum_attack_charge: Option<f32>,
    #[serde(rename = "minecraft:mob_visibility")]
    pub mob_visibility: Option<MobVisibilityComponent>,
    #[serde(rename = "minecraft:ominous_bottle_amplifier")]
    pub ominous_bottle_amplifier: Option<i32>,
    #[serde(rename = "minecraft:piercing_weapon")]
    pub piercing_weapon: Option<PiercingWeaponComponent>,
    #[serde(rename = "minecraft:pot_decorations")]
    pub pot_decorations: Option<serde_json::Value>,
    #[serde(rename = "minecraft:potion_contents")]
    pub potion_contents: Option<serde_json::Value>,
    #[serde(rename = "minecraft:potion_duration_scale")]
    pub potion_duration_scale: Option<f32>,
    #[serde(rename = "minecraft:provides_banner_patterns")]
    pub provides_banner_patterns: Option<String>,
    #[serde(rename = "minecraft:provides_pottery_pattern")]
    pub provides_pottery_pattern: Option<String>,
    #[serde(rename = "minecraft:provides_trim_material")]
    pub provides_trim_material: Option<String>,
    #[serde(rename = "minecraft:rarity")]
    pub rarity: Option<String>,
    #[serde(rename = "minecraft:recipes")]
    pub recipes: Option<serde_json::Value>,
    #[serde(rename = "minecraft:repair_cost")]
    pub repair_cost: Option<i32>,
    #[serde(rename = "minecraft:repairable")]
    pub repairable: Option<RepairableComponent>,
    #[serde(rename = "minecraft:sign_text_back")]
    pub sign_text_back: Option<serde_json::Value>,
    #[serde(rename = "minecraft:sign_text_front")]
    pub sign_text_front: Option<serde_json::Value>,
    #[serde(rename = "minecraft:stored_enchantments")]
    pub stored_enchantments: Option<serde_json::Value>,
    #[serde(rename = "minecraft:suspicious_stew_effects")]
    pub suspicious_stew_effects: Option<serde_json::Value>,
    #[serde(
        rename = "minecraft:attack_animation",
        alias = "minecraft:swing_animation"
    )]
    pub swing_animation: Option<SwingAnimationComponent>,
    #[serde(rename = "minecraft:tooltip_display")]
    pub tooltip_display: Option<serde_json::Value>,
    #[serde(rename = "minecraft:use_effects")]
    pub use_effects: Option<UseEffectsComponent>,
    #[serde(rename = "minecraft:use_remainder")]
    pub use_remainder: Option<UseRemainderComponent>,
    #[serde(rename = "minecraft:villager_food")]
    pub villager_food: Option<VillagerFoodComponent>,
    #[serde(rename = "minecraft:waxed")]
    pub waxed: Option<serde_json::Value>,
    #[serde(rename = "minecraft:writable_book_content")]
    pub writable_book_content: Option<serde_json::Value>,
}

fn default_swing_animation_type() -> String {
    "whack".to_string()
}

const fn default_swing_animation_duration() -> i32 {
    6
}

#[derive(Deserialize, Clone)]
pub struct SwingAnimationComponent {
    #[serde(default = "default_swing_animation_type")]
    pub r#type: String,
    #[serde(default = "default_swing_animation_duration")]
    pub duration: i32,
}

#[derive(Deserialize)]
pub struct EnchantableComponent {
    pub value: i32,
}

#[derive(Deserialize, Clone)]
pub struct CompostableComponent {
    pub layers: String,
}

#[derive(Deserialize, Clone)]
pub struct UseRemainderComponent {
    pub id: String,
}

const fn default_use_effects_speed() -> f32 {
    0.2
}

#[derive(Deserialize, Clone)]
pub struct UseEffectsComponent {
    #[serde(default)]
    pub can_sprint: bool,
    #[serde(default = "_true")]
    pub interact_vibrations: bool,
    #[serde(default = "default_use_effects_speed")]
    pub speed_multiplier: f32,
}

#[derive(Deserialize, Clone)]
pub struct VillagerFoodComponent {
    pub nutrition: i32,
}

#[derive(Deserialize, Clone)]
pub struct MobVisibilityComponent {
    pub targeting_entity_types: StringOrList,
    pub visibility: f32,
}

impl ToTokens for ItemComponents {
    /// Emits a sequence of `(DataComponent, &impl DataComponentImpl)` tuple expressions for code generation.
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.push_max_stack_size(tokens);

        self.push_use_cooldown(tokens);

        self.push_jukebox_playable(tokens);

        self.push_item_name(tokens);
        self.push_damage(tokens);

        self.push_max_damage(tokens);

        self.push_attribute_modifiers(tokens);

        self.push_tool(tokens);

        self.push_food(tokens);

        self.push_consumable(tokens);

        if self.blocks_attacks.is_some() {
            tokens.extend(quote! { (BlocksAttacks, &BlocksAttacksImpl), });
        }

        if self.death_protection.is_some() {
            tokens.extend(quote! { (DeathProtection, &DeathProtectionImpl), });
        }

        self.push_weapon(tokens);

        self.push_damage_resistant(tokens);

        self.push_equippable(tokens);

        self.push_enchantable(tokens);

        self.push_attack_range(tokens);
        if self.banner_patterns.is_some() {
            tokens.extend(quote! { (BannerPatterns, &BannerPatternsImpl::EMPTY), });
        }
        if self.bees.is_some() {
            tokens.extend(quote! { (Bees, &BeesImpl), });
        }
        self.push_block_state(tokens);
        self.push_block_transformer(tokens);
        self.push_break_sound(tokens);
        if self.bucket_entity_data.is_some() {
            tokens.extend(quote! { (BucketEntityData, &BucketEntityDataImpl), });
        }
        if self.bundle_contents.is_some() {
            tokens.extend(quote! { (BundleContents, &BundleContentsImpl { items: Vec::new() }), });
        }
        if self.charged_projectiles.is_some() {
            tokens.extend(quote! { (ChargedProjectiles, &ChargedProjectilesImpl { projectiles: Vec::new() }), });
        }
        self.push_chicken_variant(tokens);
        if self.container.is_some() {
            tokens.extend(quote! { (Container, &ContainerImpl { items: Vec::new() }), });
        }
        self.push_cushion_color(tokens);
        self.push_damage_type(tokens);
        if self.debug_stick_state.is_some() {
            tokens.extend(quote! { (DebugStickState, &DebugStickStateImpl), });
        }
        self.push_dye(tokens);
        if self.brewing_fuel.is_some() {
            tokens.extend(quote! { (BrewingFuel, &BrewingFuelImpl), });
        }
        self.push_compostable(tokens);
        self.push_cooking_fuel(tokens);
        if self.waxed.is_some() {
            tokens.extend(quote! { (Waxed, &WaxedImpl), });
        }
        self.push_enchantment_glint_override(tokens);
        self.push_enchantments(tokens);
        if self.entity_data.is_some() {
            tokens.extend(quote! { (EntityData, &EntityDataImpl { nbt: None }), });
        }
        self.push_fireworks(tokens);
        if self.glider.is_some() {
            tokens.extend(quote! { (Glider, &GliderImpl), });
        }
        self.push_instrument(tokens);
        self.push_interact_animation(tokens);
        self.push_item_model(tokens);
        self.push_kinetic_weapon(tokens);
        if self.lore.is_some() {
            tokens.extend(quote! { (Lore, &LoreImpl { lines: Vec::new() }), });
        }
        if self.map_decorations.is_some() {
            tokens.extend(quote! { (MapDecorations, &MapDecorationsImpl), });
        }
        self.push_minimum_attack_charge(tokens);
        self.push_mob_visibility(tokens);
        self.push_ominous_bottle_amplifier(tokens);
        self.push_piercing_weapon(tokens);
        if self.pot_decorations.is_some() {
            tokens.extend(quote! { (PotDecorations, &PotDecorationsImpl), });
        }
        self.push_potion_contents(tokens);
        self.push_potion_duration_scale(tokens);
        self.push_provides_banner_patterns(tokens);
        self.push_provides_pottery_pattern(tokens);
        self.push_provides_trim_material(tokens);
        self.push_rarity(tokens);
        if self.recipes.is_some() {
            tokens.extend(quote! { (Recipes, &RecipesImpl), });
        }
        if let Some(cost) = self.repair_cost {
            tokens.extend(quote! { (RepairCost, &RepairCostImpl { cost: #cost }), });
        }
        self.push_repairable(tokens);
        if self.sign_text_back.is_some() {
            tokens.extend(quote! { (SignTextBack, &SignTextBackImpl::DEFAULT), });
        }
        if self.sign_text_front.is_some() {
            tokens.extend(quote! { (SignTextFront, &SignTextFrontImpl::DEFAULT), });
        }
        if self.stored_enchantments.is_some() {
            tokens.extend(quote! { (StoredEnchantments, &StoredEnchantmentsImpl { enchantment: Cow::Borrowed(&[]) }), });
        }
        if self.suspicious_stew_effects.is_some() {
            tokens.extend(quote! { (SuspiciousStewEffects, &SuspiciousStewEffectsImpl::EMPTY), });
        }
        self.push_swing_animation(tokens);
        if self.tooltip_display.is_some() {
            tokens.extend(quote! { (TooltipDisplay, &TooltipDisplayImpl), });
        }
        self.push_use_effects(tokens);
        self.push_use_remainder(tokens);
        self.push_villager_food(tokens);

        self.push_writable_book_content(tokens);
    }
}

/// Serde default helper returning `1f32`.
const fn return_1f32() -> f32 {
    1.
}

/// Serde default helper returning `true`.
const fn return_true() -> bool {
    true
}

/// Deserialized tool component containing mining rules and default speed.
const fn default_item_damage() -> u32 {
    1
}

#[derive(Deserialize)]
pub struct ToolComponent {
    /// Ordered list of mining rules applied per block or block tag.
    rules: Vec<ToolRule>,
    /// Default mining speed when no rule matches, defaults to `1.0`.
    #[serde(default = "return_1f32")]
    default_mining_speed: f32,
    /// Durability consumed per block broken, defaults to `1`.
    #[serde(default = "default_item_damage")]
    damage_per_block: u32,
    /// Whether the tool can destroy blocks in creative mode, defaults to `true`.
    #[serde(default = "return_true")]
    can_destroy_blocks_in_creative: bool,
}

#[derive(Deserialize)]
pub struct UseCooldownComponent {
    seconds: f32,
    cooldown_group: Option<String>,
}

/// Serde default helper returning `false`.
const fn return_false() -> bool {
    false
}

/// Deserialized food component describing nutrition and saturation values.
#[derive(Deserialize, Copy, Clone)]
pub struct FoodComponent {
    /// Hunger points restored when eaten.
    nutrition: u8,
    /// Saturation modifier applied on eating.
    saturation: f32,
    /// Whether the item can be eaten even at full hunger, defaults to `false`.
    #[serde(default = "return_false")]
    can_always_eat: bool,
}

/// A single tool mining rule that maps a block set to an optional speed override.
#[derive(Deserialize, Clone)]
pub struct ToolRule {
    /// The block or block-tag set this rule applies to.
    blocks: RegistryEntryList,
    /// Optional speed override when mining matching blocks.
    speed: Option<f32>,
    /// Whether the tool yields drops for matching blocks, if specified.
    correct_for_drops: Option<bool>,
}

/// A single attribute modifier applied when the item is held or worn.
#[derive(Deserialize, Clone)]
pub struct Modifier {
    /// Namespaced attribute key (e.g., `"minecraft:attack_damage"`).
    pub r#type: String,
    /// Unique identifier for this modifier instance.
    pub id: String,
    /// Numeric value added or multiplied depending on `operation`.
    pub amount: f64,
    /// How `amount` is combined with the base attribute value.
    pub operation: Operation,
    // TODO: Make this an enum
    /// Equipment slot in which this modifier is active.
    pub slot: AttributeModifierSlot,
}

/// Serde default helper returning `true`.
const fn _true() -> bool {
    true
}

/// Deserialized consumable component describing use duration.
#[derive(Deserialize, Clone)]
pub struct Consumable {
    consume_seconds: Option<f32>,
    has_consume_particles: Option<bool>,
    animation: Option<String>,
    sound: Option<String>,
    on_consume_effects: Option<Vec<ConsumeEffect>>,
}
#[derive(Deserialize, Clone)]
pub struct ConsumeEffect {
    r#type: String,
    probability: Option<f32>,
    diameter: Option<f32>,
    sound: Option<String>,
    effects: Option<StringOrStatusEffects>,
}
#[derive(Deserialize, Clone)]
pub struct StatusEffectInstance {
    pub id: String,
    pub amplifier: Option<i32>,
    pub duration: Option<i32>,
    pub ambient: Option<bool>,
    pub show_particles: Option<bool>,
    pub show_icon: Option<bool>,
}
#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum StringOrStatusEffects {
    String(String),
    Effects(Vec<StatusEffectInstance>),
}

/// Deserialized death-protection component (e.g., totem of undying); fields are unimplemented.
#[derive(Deserialize, Clone)]
pub struct DeathProtection {
    // TODO
}

/// Deserialized attack-blocking component (e.g., shield); fields are unimplemented.
#[derive(Deserialize, Clone)]
pub struct WeaponComponent {
    #[serde(default = "default_item_damage")]
    pub item_damage_per_attack: u32,
    #[serde(default)]
    pub disable_blocking_for_seconds: f32,
}

#[derive(Deserialize, Clone)]
pub struct AttackRangeComponent {
    #[serde(default)]
    pub min_reach: f32,
    #[serde(default = "default_max_reach")]
    pub max_reach: f32,
    #[serde(default)]
    pub min_creative_reach: f32,
    #[serde(default = "default_max_creative_reach")]
    pub max_creative_reach: f32,
    #[serde(default = "default_hitbox_margin")]
    pub hitbox_margin: f32,
    #[serde(default = "return_1f32")]
    pub mob_factor: f32,
}

const fn default_max_reach() -> f32 {
    3.0
}

const fn default_max_creative_reach() -> f32 {
    5.0
}

const fn default_hitbox_margin() -> f32 {
    0.3
}

#[derive(Deserialize, Clone)]
pub struct KineticConditionComponent {
    pub max_duration_ticks: i32,
    #[serde(default)]
    pub min_speed: f32,
    #[serde(default)]
    pub min_relative_speed: f32,
}

#[derive(Deserialize, Clone)]
pub struct KineticWeaponComponent {
    #[serde(default = "default_contact_cooldown_ticks")]
    pub contact_cooldown_ticks: i32,
    #[serde(default)]
    pub delay_ticks: i32,
    pub dismount_conditions: Option<KineticConditionComponent>,
    pub knockback_conditions: Option<KineticConditionComponent>,
    pub damage_conditions: Option<KineticConditionComponent>,
    #[serde(default)]
    pub forward_movement: f32,
    #[serde(default = "return_1f32")]
    pub damage_multiplier: f32,
    pub sound: Option<String>,
    pub hit_sound: Option<String>,
}

const fn default_contact_cooldown_ticks() -> i32 {
    10
}

#[derive(Deserialize, Clone)]
pub struct PiercingWeaponComponent {
    #[serde(default = "_true")]
    pub deals_knockback: bool,
    #[serde(default)]
    pub dismounts: bool,
    pub sound: Option<String>,
    pub hit_sound: Option<String>,
}

fn float_literal(value: f32) -> LitFloat {
    LitFloat::new(&format!("{value:?}f32"), Span::call_site())
}

fn optional_sound_tokens(sound: Option<&str>) -> TokenStream {
    sound.map_or_else(
        || quote! { None },
        |sound| {
            let variant = format_ident!(
                "{}",
                sound
                    .strip_prefix("minecraft:")
                    .unwrap_or(sound)
                    .to_pascal_case()
            );
            quote! { Some(IdOr::Id(Sound::#variant)) }
        },
    )
}

fn kinetic_condition_tokens(condition: Option<&KineticConditionComponent>) -> TokenStream {
    condition.map_or_else(
        || quote! { None },
        |condition| {
            let max_duration_ticks =
                LitInt::new(&condition.max_duration_ticks.to_string(), Span::call_site());
            let min_speed = float_literal(condition.min_speed);
            let min_relative_speed = float_literal(condition.min_relative_speed);
            quote! {
                Some(KineticConditionImpl {
                    max_duration_ticks: #max_duration_ticks,
                    min_speed: #min_speed,
                    min_relative_speed: #min_relative_speed,
                })
            }
        },
    )
}

#[derive(Deserialize, Clone)]
pub struct BlocksAttacks {
    // TODO
}

/// Deserialized damage-resistance component indicating which damage types the item resists.
#[derive(Deserialize, Clone)]
pub struct DamageResistantComponent {
    /// Namespaced damage type tag the item is immune to.
    pub types: String,
}
#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum StringOrList {
    String(String),
    List(Vec<String>),
}

/// An entity type `IDSet`: one type, a list, or a `#tag`; `minecraft:` is optional.
fn entity_idset_tokens(entities: &StringOrList) -> TokenStream {
    let entity = |name: &str| {
        let ident = format_ident!(
            "{}",
            name.strip_prefix("minecraft:")
                .unwrap_or(name)
                .to_uppercase()
        );
        quote! { &crate::entity_type::EntityType::#ident, }
    };
    match entities {
        StringOrList::String(str) => {
            if let Some(tag) = str.strip_prefix('#') {
                let tag = tag.strip_prefix("minecraft:").unwrap_or(tag);
                quote! { IDSet::Tag(Cow::Borrowed(#tag)) }
            } else {
                let ids = entity(str);
                quote! { IDSet::IDs(Cow::Borrowed(&[#ids])) }
            }
        }
        StringOrList::List(items) => {
            let ids: TokenStream = items.iter().map(|name| entity(name)).collect();
            quote! { IDSet::IDs(Cow::Borrowed(&[#ids])) }
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct CookingFuelComponent {
    pub burn_time: serde_json::Value,
    #[serde(default)]
    pub speed_multiplier: Option<serde_json::Value>,
}

#[derive(Deserialize, Clone)]
pub struct RepairableComponent {
    pub items: StringOrList,
}

/// Deserialized equippable component describing how an item is worn or equipped.
#[derive(Deserialize, Clone)]
pub struct EquippableComponent {
    /// Equipment slot the item occupies (e.g., `"head"`, `"chest"`).
    pub slot: String,
    /// Sound event played when the item is equipped; uses a generic fallback if absent.
    pub equip_sound: Option<String>,
    /// Texture asset identifier for the equipped model, if any.
    pub asset_id: Option<String>,
    /// Screen overlay texture shown while equipped, if any.
    pub camera_overlay: Option<String>,
    pub allowed_entities: Option<StringOrList>,
    #[serde(default = "_true")]
    pub dispensable: bool,
    /// Whether shift-clicking swaps the item into the equipment slot, defaults to `true`.
    #[serde(default = "_true")]
    pub swappable: bool,
    /// Whether the item takes damage when the wearer is hurt, defaults to `true`.
    #[serde(default = "_true")]
    pub damage_on_hurt: bool,
    /// Whether right-clicking an entity equips the item, defaults to `false`.
    #[serde(default)]
    pub equip_on_interact: bool,
    /// Whether shears can remove this item from an entity, defaults to `false`.
    #[serde(default)]
    pub can_be_sheared: bool,
    /// Sound event played when sheared off, if any.
    pub shearing_sound: Option<String>,
}

/// Arithmetic operation applied when combining an attribute modifier's amount with the base value.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[expect(clippy::enum_variant_names)]
pub enum Operation {
    /// Adds the modifier amount directly to the base value.
    AddValue,
    /// Adds `amount * base` to the running total.
    AddMultipliedBase,
    /// Multiplies the running total by `1 + amount`.
    AddMultipliedTotal,
}

#[derive(Deserialize_repr, Debug)]
#[repr(i32)]
enum BedrockItemVersion {
    Legacy = 0,
    DataDriven = 1,
    None = 2,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BedrockRuntimeItemState {
    name: String,
    id: i16,
    version: BedrockItemVersion,
    component_based: bool,
}

#[derive(Debug)]
enum BedrockToolType {
    Sword,
    Shovel,
    Pickaxe,
    Axe,
    Shears,
    Hoe,
}

#[derive(Debug)]
enum BedrockArmorType {
    Helmet,
    Leggings,
    Chestplate,
    Boots,
}

#[derive(Debug)]
struct BedrockRemapEntry {
    identifier: String,
    data: u32,
    block_runtime_id_range: Option<(i32, i32)>,
    tool_type: Option<BedrockToolType>,
    armor_type: Option<BedrockArmorType>,
    protection_value: Option<i32>,
    is_entity_placer: bool,
    is_edible: bool,
}

#[derive(Debug)]
pub struct BedrockItem {
    id: i16,
    registry_key: String,
    version: BedrockItemVersion,
    component_based: bool,
    components: Nbt,
}

/// Reads `items.json` and generates the complete item registry `TokenStream`.
pub fn build() -> TokenStream {
    let blocks_assets: BlockAssets =
        serde_json::from_str(&fs::read_to_string("../../assets/blocks.json").unwrap())
            .expect("Failed to parse blocks.json");

    let items: BTreeMap<String, Item> =
        serde_json::from_str(&fs::read_to_string("../../assets/items.json").unwrap())
            .expect("Failed to parse items.json");

    let be_item_components = load_bedrock_item_components();

    let be_runtime_item_states: Vec<BedrockRuntimeItemState> = serde_json::from_str(
        &fs::read_to_string("../../assets/bedrock/runtime_item_states.json").unwrap(),
    )
    .expect("Failed to parse bedrock/runtime_item_states.json");

    let be_item_data_overrides: BTreeMap<String, u32> = serde_json::from_str(
        &fs::read_to_string("../../assets/bedrock/item_data_overrides.json").unwrap(),
    )
    .expect("Failed to parse bedrock/item_data_overrides.json");

    let be_valid_item_identifiers = be_runtime_item_states
        .iter()
        .map(|item| &item.name)
        .collect::<HashSet<_>>();

    let be_item_remaps = java_to_bedrock_remaps(
        &items,
        &blocks_assets,
        &be_valid_item_identifiers,
        &be_item_data_overrides,
    );

    let bedrock_item_definitions =
        bedrock_item_definitions(be_runtime_item_states, &be_item_components);

    let JavaItemTokens {
        constants,
        type_from_raw_id_arms,
        type_from_name,
        all_java_items,
        all_java_item_ids,
    } = java_item_tokens(&items);

    let bedrock_id_by_key: std::collections::HashMap<String, i16> = bedrock_item_definitions
        .iter()
        .map(|item| (item.registry_key.clone(), item.id))
        .collect();

    let (bedrock_constants, all_bedrock_items) = bedrock_item_constants(bedrock_item_definitions);
    let all_bedrock_items_count_lit =
        LitInt::new(&all_bedrock_items.len().to_string(), Span::call_site());

    let mapping_constants = mapping_constants(&be_item_remaps);
    let (bedrock_to_java_arms, bedrock_id_fallback_arms) =
        bedrock_to_java_arms(&be_item_remaps, &bedrock_id_by_key);

    let item_tokens = static_items::item_tokens();
    let java_to_bedrock_item_mapping_tokens = static_items::java_to_bedrock_item_mapping_tokens();
    let translated_name_tokens = static_items::translated_name_tokens();
    let item_tokens_2 = static_items::item_tokens_2();
    quote! {
        #item_tokens

        #java_to_bedrock_item_mapping_tokens

        impl Item {
            #constants

            #translated_name_tokens

            #[doc = "Try to parse an item from a resource location string."]
            #[must_use]
            pub fn from_registry_key(name: &str) -> Option<&'static Self> {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                match name {
                    #type_from_name
                    _ => None
                }
            }

            #[doc = "Try to parse an item from a raw id."]
            #[must_use]
            pub const fn from_id(id: u16) -> Option<&'static Self> {
                match id {
                    #type_from_raw_id_arms
                    _ => None
                }
            }
        }

        #item_tokens_2

        impl BedrockItem {
            #bedrock_constants

            pub const ALL_BEDROCK_ITEMS: [&'static Self; #all_bedrock_items_count_lit] = [
                #( &Self::#all_bedrock_items, )*
            ];
        }

        impl JavaToBedrockItemMapping {
            #mapping_constants

            #[must_use]
            pub const fn from_java_item_id(item_id: u16) -> Option<&'static Self> {
                match item_id {
                    #( #all_java_item_ids => Some(&Self::#all_java_items), )*
                    _ => None,
                }
            }

            #[must_use]
            pub fn from_bedrock(bedrock_id: i16, bedrock_data: u32) -> Option<&'static Self> {
                match (bedrock_id, bedrock_data) {
                    #bedrock_to_java_arms
                    _ => match bedrock_id {
                        #bedrock_id_fallback_arms
                        _ => None,
                    }
                }
            }
        }
    }
}
