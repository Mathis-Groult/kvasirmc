mod property_value;
mod registry;
mod sections;
mod static_items;

use property_value::property_value_arms;
use registry::{
    BlockContext, BlockTables, PropertyGroupArms, StateLists, property_enum_structs,
    property_group_arms, push_block, shape_offset_arms,
};
use sections::{
    block_id_impl_tokens, block_impl_tokens, mappings_tokens, property_enum_tokens,
    property_key_tokens, state_table_tokens,
};
mod property_struct;

use heck::{ToPascalCase, ToShoutySnakeCase, ToUpperCamelCase};
use proc_macro2::{Span, TokenStream};
use property_struct::block_properties_impl;
use pumpkin_nbt::deserializer::{NbtReadHelper, NbtReadHelperBedrock};
use pumpkin_util::math::{experience::Experience, vector3::Vector3};
use quote::{ToTokens, format_ident, quote};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read, Seek, SeekFrom},
    panic,
};
use syn::{Ident, LitInt, LitStr};

use crate::bitsets::{Bitset, gen_u16_bitset};

/// Converts a sparse index-value list into a dense array, filling gaps with `None` tokens.
///
/// # Arguments
/// – `array` – pairs of `(index, value)` where `index` is the position in the output array.
fn fill_array<T: Clone + ToTokens>(array: Vec<(u16, T)>) -> Vec<TokenStream> {
    let max_index = array.iter().map(|(index, _)| index).max().unwrap();
    let mut raw_id_from_state_id_ordered = vec![quote! { None }; (max_index + 1) as usize];

    for (state_id, id_lit) in array {
        raw_id_from_state_id_ordered[state_id as usize] = quote! { #id_lit };
    }

    raw_id_from_state_id_ordered
}

/// Converts a sparse `(block_ident, state_index, state_id)` list into a dense `TokenStream` array.
///
/// # Arguments
/// – `array` – triples of `(block_ident, state_index, state_id)` mapping each state ID to the block constant and local index.
fn fill_state_array(array: Vec<(Ident, usize, u16)>) -> Vec<TokenStream> {
    let max_index = array.iter().map(|(_, _, index)| index).max().unwrap();
    let mut ret = vec![quote! { Missed State }; (max_index + 1) as usize];

    for (block, index, state_id) in array {
        let index_lit = LitInt::new(&index.to_string(), Span::call_site());
        ret[state_id as usize] = quote! { &Block::#block.states[#index_lit] };
    }

    ret
}

/// Converts a block registry name into the SCREAMING_SNAKE_CASE constant identifier.
///
/// # Arguments
/// – `block` – the lowercase registry name (e.g., `"stone_slab"`).
fn const_block_name_from_block_name(block: &str) -> String {
    block.to_shouty_snake_case()
}

/// Derives the UpperCamelCase struct name for a block-property group from a derived base name.
///
/// # Arguments
/// – `name` – a base name such as `"oak_slab_like"` which is converted to e.g., `"OakSlabLikeProperties"`.
fn property_group_name_from_derived_name(name: &str) -> String {
    format!("{name}_properties").to_upper_camel_case()
}

fn common_suffix_group_alias(blocks: &[(String, u16)]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }
    let token_lists: Vec<Vec<&str>> = blocks
        .iter()
        .map(|(name, _)| name.split('_').collect())
        .collect();

    let first = &token_lists[0];
    let mut common_suffix_len = 0;

    for i in 1..=first.len() {
        let candidate_suffix = &first[first.len() - i..];
        let all_match = token_lists
            .iter()
            .all(|tokens| tokens.len() >= i && &tokens[tokens.len() - i..] == candidate_suffix);
        if all_match {
            common_suffix_len = i;
        } else {
            break;
        }
    }

    if common_suffix_len > 0 {
        let suffix_tokens = &first[first.len() - common_suffix_len..];
        let suffix_str = suffix_tokens.join("_");
        Some(format!("{suffix_str}_properties").to_upper_camel_case())
    } else {
        None
    }
}

/// Discriminates between the runtime representations of a block property.
enum PropertyType {
    /// The property is a simple boolean (`true`/`false`).
    Bool,
    /// The property is an integer with an inclusive range.
    Int { min: u8, max: u8 },
    /// The property is an enum with a generated Rust type identified by `name`.
    Enum {
        name: String,
        values: Vec<String>,
        prop_variant_name: String,
        canonical_enum_name: String,
        shared_none_variants: Vec<(String, String)>,
    },
}

/// Maps a single block-property field to its Rust type representation.
struct PropertyVariantMapping {
    /// The serialized property name as it appears in JSON (e.g., `"facing"`).
    original_name: String,
    /// The Rust type used to represent this property in the generated code.
    property_type: PropertyType,
}

/// Accumulated data for a group of blocks that share the same set of block properties.
struct PropertyCollectionData {
    /// The ordered list of property-to-type mappings that form the generated struct fields.
    variant_mappings: Vec<PropertyVariantMapping>,
    /// All blocks (by name and numeric ID) that belong to this property group.
    blocks: Vec<(String, u16)>,
}

impl PropertyCollectionData {
    /// Registers an additional block that shares this property group.
    ///
    /// # Arguments
    /// – `block_name` – the registry name of the block.
    /// – `block_id` – the numeric block ID.
    pub fn add_block(&mut self, block_name: String, block_id: u16) {
        self.blocks.push((block_name, block_id));
    }

    /// Creates a new `PropertyCollectionData` from an ordered list of property mappings with no blocks yet registered.
    ///
    /// # Arguments
    /// – `variant_mappings` – the property-to-type mappings for this group.
    pub const fn from_mappings(variant_mappings: Vec<PropertyVariantMapping>) -> Self {
        Self {
            variant_mappings,
            blocks: Vec::new(),
        }
    }

    /// Derives the base name for this property group from the first registered block.
    pub fn derive_name(&self) -> String {
        format!("{}_like", self.blocks[0].0)
    }
}

/// Deserialized representation of a single block property enum, used to emit the Rust enum definition.
#[derive(Deserialize, Clone, Debug)]
pub struct PropertyStruct {
    /// The UpperCamelCase name used for the generated Rust enum (e.g., `"Facing"`).
    pub name: String,
    /// The ordered list of variant value strings (e.g., `["north", "south", ...]`).
    pub values: Vec<String>,
}

impl ToTokens for PropertyStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if self.values[0] == "true" && self.values[1] == "false" {
            return;
        }

        let name = Ident::new(&self.name, Span::call_site());
        let count = self.values.len();
        let variant_count = count as u16;

        let is_number_values = !self.values.is_empty()
            && self.values.iter().all(|v| v.starts_with('L'))
            && self.values.iter().any(|v| v == "L1");

        let mut variants = Vec::with_capacity(count);
        let mut literals = Vec::with_capacity(count);
        let mut indices = Vec::with_capacity(count);

        for (i, raw_value) in self.values.iter().enumerate() {
            let ident = Ident::new(&raw_value.to_upper_camel_case(), Span::call_site());

            let literal_str = if is_number_values {
                raw_value.strip_prefix('L').unwrap_or(raw_value)
            } else {
                raw_value.as_str()
            };

            variants.push(ident);
            literals.push(literal_str);
            indices.push(i as u16);
        }
        tokens.extend(quote! {
            #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
            pub enum #name {
                #(#variants),*
            }

            impl #name {
                #[must_use]
                pub const fn to_value(&self) -> &'static str {
                    match self {
                        #(Self::#variants => #literals),*
                    }
                }
            }

            impl EnumVariants for #name {
                fn variant_count() -> u16 {
                    #variant_count
                }

                fn to_index(&self) -> u16 {
                    match self {
                        #(Self::#variants => #indices),*
                    }
                }

                fn from_index(index: u16) -> Self {
                    match index {
                        #(#indices => Self::#variants,)*
                        _ => panic!("Invalid index: {index}"),
                    }
                }

                fn to_value(&self) -> &'static str {
                    self.to_value()
                }

                fn from_value(value: &str) -> Self {
                    match value {
                        #(#literals => Self::#variants),*,
                        _ => panic!("Invalid value: {value}"),
                    }
                }
            }
        });
    }
}

/// Code-generation wrapper that emits the full `BlockProperties` impl for one property group.
struct BlockPropertyStruct {
    /// The property group data used to generate the struct and its trait implementation.
    data: PropertyCollectionData,
    /// Type aliases for this property struct.
    aliases: Vec<Ident>,
}

impl ToTokens for BlockPropertyStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let struct_name = property_group_name_from_derived_name(&self.data.derive_name());
        let name = Ident::new(&struct_name, Span::call_site());

        // Strukturfelder generieren
        let fields = self.field_tokens();

        let block_ids = self.data.blocks.iter().map(|(name, _)| {
            Ident::new(&const_block_name_from_block_name(name), Span::call_site())
        });

        let to_index_logic = self.data.variant_mappings.iter().rev().map(|entry| {
            let field = Ident::new_raw(&entry.original_name, Span::call_site());
            match &entry.property_type {
                PropertyType::Bool => quote! { (!self.#field as u16, 2) },
                PropertyType::Int { min, max } => {
                    let count = (max - min + 1) as u16;

                    if *min > 0 {
                        quote! { ((self.#field - #min) as u16, #count) }
                    } else {
                        quote! { (self.#field as u16, #count) }
                    }
                }
                PropertyType::Enum { name, .. } => {
                    let ty = Ident::new(name, Span::call_site());
                    quote! { (self.#field.to_index(), #ty::variant_count()) }
                }
            }
        });

        let from_index_body = self.from_index_body();

        let to_props_entries = self.to_props_entries();

        let from_props_arms = self.from_props_arms();

        let aliases = self.aliases.iter().map(|alias| {
            quote! { pub type #alias = #name; }
        });

        tokens.extend(quote! {
            #[derive(Clone, Copy, Eq, PartialEq)]
            pub struct #name {
                #(#fields),*
            }

            #(#aliases)*

            impl #name {
                #[inline]
                #[must_use]
                pub fn from_index(mut index: u16) -> Self {
                    Self {
                        #(#from_index_body),*
                    }
                }

                #[inline]
                #[must_use]
                pub fn from_state_id(id: BlockStateId) -> Self {
                    let block = Block::from_state_id(id);
                    let min_id = block.states[0].id.as_u16();
                    Self::from_index(id.as_u16() - min_id)
                }

                #[inline]
                #[must_use]
                pub fn to_state_id(&self, block: &Block) -> BlockStateId {
                    <Self as BlockProperties>::to_state_id(self, block)
                }

                #[inline]
                #[must_use]
                pub fn default(block: &Block) -> Self {
                    <Self as BlockProperties>::default(block)
                }
            }
        });
        tokens.extend(block_properties_impl(
            &name,
            &struct_name,
            to_index_logic,
            block_ids,
            to_props_entries,
            &from_props_arms,
        ));
    }
}

/// Deserialized flammability data for a block.
#[derive(Deserialize)]
pub struct FlammableStruct {
    /// Chance (0–300) that fire spreads to neighbouring blocks.
    pub spread_chance: u8,
    /// Chance (0–300) that the block itself burns away.
    pub burn_chance: u8,
}

impl ToTokens for FlammableStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let spread_chance = &self.spread_chance;
        let burn_chance = &self.burn_chance;

        tokens.extend(quote! {
            Flammable {
                spread_chance: #spread_chance,
                burn_chance: #burn_chance,
            }
        });
    }
}

/// Axis-aligned bounding box used to describe block collision and outline shapes.
#[derive(Deserialize, Clone, Copy)]
pub struct BoundingBox {
    /// The minimum corner of the bounding box.
    pub min: Vector3<f64>,
    /// The maximum corner of the bounding box.
    pub max: Vector3<f64>,
}

impl ToTokens for BoundingBox {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let min_x = &self.min.x;
        let min_y = &self.min.y;
        let min_z = &self.min.z;

        let max_x = &self.max.x;
        let max_y = &self.max.y;
        let max_z = &self.max.z;

        tokens.extend(quote! {
            BoundingBox {
                min: Vector3::new(#min_x, #min_y, #min_z),
                max: Vector3::new(#max_x, #max_y, #max_z),
            }
        });
    }
}

#[derive(Deserialize, Copy, Clone, PartialEq, Eq)]
pub struct BlockStateId(pub u16);

impl ToTokens for BlockStateId {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let inner = self.0;
        tokens.extend(quote! { BlockStateId::new(#inner).unwrap() });
    }
}

/// Deserialized representation of a single block state as stored in `blocks.json`.
#[derive(Deserialize, Clone)]
pub struct BlockState {
    /// Globally unique numeric ID for this block state.
    pub id: BlockStateId,
    /// Bitfield encoding boolean state properties (air, random ticks, etc.).
    pub state_flags: u16,
    /// Bitfield encoding which sides of the block are solid.
    pub side_flags: u8,
    /// Name of the note-block instrument for this state. TODO: make this an enum
    pub instrument: String,
    /// Light level emitted by this state (0–15).
    pub luminance: u8,
    /// How pistons interact with this block state.
    pub piston_behavior: PistonBehavior,
    /// Mining hardness of this block state.
    pub hardness: f32,
    /// Indices into the global shapes array for collision shape segments.
    pub collision_shapes: Vec<u16>,
    /// Indices into the global shapes array for outline (selection) shape segments.
    pub outline_shapes: Vec<u16>,
    /// Opacity value for light propagation, if non-full.
    pub opacity: Option<u8>,
    /// Index into the global sound type array.
    pub sound_type: u8,
    /// Associated block-entity type ID, if any.
    pub block_entity_type: Option<u16>,
}

/// Describes how a piston interacts with a block.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PistonBehavior {
    /// The block can be pushed and pulled normally.
    #[serde(alias = "PUSH_PULL")]
    Normal,
    /// The block is destroyed when pushed.
    #[serde(alias = "POPPED")]
    Destroy,
    /// The block prevents piston movement.
    Block,
    /// The piston ignores the block entirely.
    #[serde(alias = "IMMOVEABLE")]
    Ignore,
    /// The block can only be pushed, not pulled.
    #[serde(alias = "PUSH")]
    PushOnly,
}

impl PistonBehavior {
    fn to_tokens(&self) -> TokenStream {
        match self {
            Self::Normal => quote! { PistonBehavior::Normal },
            Self::Destroy => quote! { PistonBehavior::Destroy },
            Self::Block => quote! { PistonBehavior::Block },
            Self::Ignore => quote! { PistonBehavior::Ignore },
            Self::PushOnly => quote! { PistonBehavior::PushOnly },
        }
    }
}

#[derive(Deserialize, Copy, Clone)]
#[serde(rename_all = "snake_case")]
enum SpawnFloorPredicate {
    Never,
    Always,
    OcelotOrParrot,
    PolarBear,
    FireImmune,
}

impl ToTokens for SpawnFloorPredicate {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match self {
            Self::Never => quote! { SpawnFloorPredicate::Never },
            Self::Always => quote! { SpawnFloorPredicate::Always },
            Self::OcelotOrParrot => quote! { SpawnFloorPredicate::OcelotOrParrot },
            Self::PolarBear => quote! { SpawnFloorPredicate::PolarBear },
            Self::FireImmune => quote! { SpawnFloorPredicate::FireImmune },
        });
    }
}

#[derive(Deserialize, Copy, Clone)]
#[serde(rename_all = "snake_case")]
enum SuffocationPredicate {
    Never,
    Always,
}

impl ToTokens for SuffocationPredicate {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match self {
            Self::Never => quote! { SuffocationPredicate::Never },
            Self::Always => quote! { SuffocationPredicate::Always },
        });
    }
}

impl BlockState {
    /// Bit flag indicating this state is an air block.
    const IS_AIR: u16 = 1 << 0;

    const IS_LIQUID: u16 = 1 << 5;

    /// Bit flag indicating this state receives random tick events.
    const HAS_RANDOM_TICKS: u16 = 1 << 9;

    const IS_SOLID_RENDER: u16 = 1 << 10;
    const CAN_OCCLUDE: u16 = 1 << 11;
    const HAS_ANALOG_OUTPUT_SIGNAL: u16 = 1 << 12;

    /// Returns `true` if this state receives random tick events.
    const fn has_random_ticks(&self) -> bool {
        self.state_flags & Self::HAS_RANDOM_TICKS != 0
    }

    /// Returns `true` if this state is an air variant.
    pub const fn is_air(&self) -> bool {
        self.state_flags & Self::IS_AIR != 0
    }

    pub const fn is_liquid(&self) -> bool {
        self.state_flags & Self::IS_LIQUID != 0
    }

    pub const fn is_solid_render(&self) -> bool {
        self.state_flags & Self::IS_SOLID_RENDER != 0
    }

    pub const fn can_occlude(&self) -> bool {
        self.state_flags & Self::CAN_OCCLUDE != 0
    }

    pub const fn has_analog_output_signal(&self) -> bool {
        self.state_flags & Self::HAS_ANALOG_OUTPUT_SIGNAL != 0
    }

    /// Emits the `BlockState { … }` struct literal token stream for code generation.
    fn to_tokens(&self) -> TokenStream {
        let mut tokens = TokenStream::new();
        let id = self.id;
        let state_flags = LitInt::new(&self.state_flags.to_string(), Span::call_site());
        let side_flags = LitInt::new(&self.side_flags.to_string(), Span::call_site());
        let instrument = format_ident!("{}", self.instrument.to_upper_camel_case());
        let luminance = LitInt::new(&self.luminance.to_string(), Span::call_site());
        let hardness = self.hardness;
        let opacity = if let Some(opacity) = self.opacity {
            let opacity = LitInt::new(&opacity.to_string(), Span::call_site());
            quote! { #opacity }
        } else {
            quote! { 0 }
        };
        let block_entity_type = if let Some(block_entity_type) = self.block_entity_type {
            let block_entity_type = LitInt::new(&block_entity_type.to_string(), Span::call_site());
            quote! { #block_entity_type }
        } else {
            quote! { u16::MAX }
        };

        let collision_shapes = self
            .collision_shapes
            .iter()
            .map(|shape_id| LitInt::new(&shape_id.to_string(), Span::call_site()));
        let outline_shapes = self
            .outline_shapes
            .iter()
            .map(|shape_id| LitInt::new(&shape_id.to_string(), Span::call_site()));
        let piston_behavior = &self.piston_behavior.to_tokens();
        let sound_type = LitInt::new(&self.sound_type.to_string(), Span::call_site());

        tokens.extend(quote! {
            BlockState {
                id: #id,
                state_flags: #state_flags,
                side_flags: #side_flags,
                instrument: NoteblockInstrument::#instrument,
                luminance: #luminance,
                piston_behavior: #piston_behavior,
                hardness: #hardness,
                collision_shapes: &[#(#collision_shapes),*],
                outline_shapes: &[#(#outline_shapes),*],
                opacity: #opacity,
                sound_type: #sound_type,
                block_entity_type: #block_entity_type,
            }
        });
        tokens
    }
}

/// Vanilla `SoundType` as stored in the `sound_types` table of `blocks.json`.
#[derive(Deserialize)]
pub struct SoundType {
    name: String,
    volume: f32,
    pitch: f32,
    break_sound: String,
    step_sound: String,
    place_sound: String,
    hit_sound: String,
    fall_sound: String,
}

impl ToTokens for SoundType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = &self.name;
        let volume = self.volume;
        let pitch = self.pitch;
        let [break_sound, step_sound, place_sound, hit_sound, fall_sound] = [
            &self.break_sound,
            &self.step_sound,
            &self.place_sound,
            &self.hit_sound,
            &self.fall_sound,
        ]
        .map(|sound| format_ident!("{}", sound.to_pascal_case()));
        tokens.extend(quote! {
            SoundType {
                name: #name,
                volume: #volume,
                pitch: #pitch,
                break_sound: Sound::#break_sound,
                step_sound: Sound::#step_sound,
                place_sound: Sound::#place_sound,
                hit_sound: Sound::#hit_sound,
                fall_sound: Sound::#fall_sound,
            }
        });
    }
}

#[derive(Deserialize, Copy, Clone)]
pub struct BlockId(pub u16);

impl ToTokens for BlockId {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let inner = self.0;
        tokens.extend(quote! { BlockId::new(#inner).unwrap() });
    }
}

/// Deserialized representation of a Minecraft block as stored in `blocks.json`.
#[derive(Deserialize)]
pub struct Block {
    /// Numeric block ID used in the protocol.
    pub id: BlockId,
    /// Registry name without the `minecraft:` namespace prefix.
    pub name: String,
    /// Translation key for the block's display name.
    // pub translation_key: String,
    /// Mining hardness; affects how long the block takes to break.
    pub hardness: f32,
    /// Blast resistance against explosions.
    pub blast_resistance: f32,
    pub map_color: u8,
    /// Numeric ID of the corresponding item, if any.
    pub item_id: u16,
    /// Flammability data, present only if the block can catch fire.
    pub flammable: Option<FlammableStruct>,
    /// Friction applied to entities walking on this block.
    pub slipperiness: f32,
    /// Horizontal velocity multiplier for entities inside this block.
    pub velocity_multiplier: f32,
    /// Jump velocity multiplier applied when jumping from this block.
    pub jump_velocity_multiplier: f32,
    /// Hash keys referencing the properties defined for this block.
    pub properties: Vec<i32>,
    /// State ID of the default (canonical) block state.
    pub default_state_id: BlockStateId,
    /// All possible states for this block in state-ID order.
    pub states: Vec<BlockState>,
    /// Experience points dropped when the block is mined, if any.
    pub experience: Option<Experience>,
    /// Loot table identifier
    pub loot_table: Option<String>,
    /// Position-derived shape offset applied by vanilla, if any.
    shape_offset: Option<BlockShapeOffset>,
    /// Spawn floor predicate for this block, if any.
    spawn_floor_predicate: Option<SpawnFloorPredicate>,
    /// Suffocation predicate for this block, if it differs from the default.
    suffocation_predicate: Option<SuffocationPredicate>,
}

impl ToTokens for Block {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let id = self.id;
        let name = LitStr::new(&self.name, Span::call_site());
        //let translation_key = LitStr::new(&self.translation_key, Span::call_site());
        let hardness = &self.hardness;
        let blast_resistance = &self.blast_resistance;
        let map_color = &self.map_color;

        let item_id = LitInt::new(&self.item_id.to_string(), Span::call_site());
        let slipperiness = &self.slipperiness;
        let velocity_multiplier = &self.velocity_multiplier;
        let jump_velocity_multiplier = &self.jump_velocity_multiplier;
        let experience = if let Some(exp) = &self.experience {
            let exp_tokens = exp.to_token_stream();
            quote! { Some(#exp_tokens) }
        } else {
            quote! { None }
        };
        let loot_table = if let Some(loot) = &self.loot_table {
            let loot_lit = LitStr::new(loot, Span::call_site());
            quote! { Some(#loot_lit) }
        } else {
            quote! { None }
        };
        // Generate state tokens
        let states = self.states.iter().map(BlockState::to_tokens);

        let default_state_ref: &BlockState = self
            .states
            .iter()
            .find(|state| state.id == self.default_state_id)
            .unwrap();
        let mut default_state = default_state_ref.clone();
        default_state.id = default_state_ref.id;
        let default_state = default_state.to_tokens();
        let flammable = if let Some(flammable) = &self.flammable {
            let flammable_tokens = flammable.to_token_stream();
            quote! { Some(#flammable_tokens) }
        } else {
            quote! { None }
        };
        tokens.extend(quote! {
            Block {
                id: #id,
                name: #name,
                hardness: #hardness,
                blast_resistance: #blast_resistance,
                map_color: #map_color,
                slipperiness: #slipperiness,
                velocity_multiplier: #velocity_multiplier,
                jump_velocity_multiplier: #jump_velocity_multiplier,
                item_id: #item_id,
                default_state: &#default_state,
                states: &[#(#states),*],
                flammable: #flammable,
                experience: #experience,
                loot_table: #loot_table,
            }
        });
    }
}

/// The underlying data type of generated block property as classified in `properties.json`.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum GeneratedPropertyType {
    /// A simple `true`/`false` boolean property.
    #[serde(rename = "boolean")]
    Boolean,
    /// An integer property with an inclusive range.
    #[serde(rename = "int")]
    Int {
        /// Minimum value (inclusive).
        min: u8,
        /// Maximum value (inclusive).
        max: u8,
    },
    /// A named-variant enum property.
    #[serde(rename = "enum")]
    Enum {
        /// The ordered list of variant strings.
        values: Vec<String>,
    },
}

/// A single block property entry as deserialized from `properties.json`.
#[derive(Deserialize, Clone)]
pub struct GeneratedProperty {
    /// A stable integer hash used to identify this property across blocks.
    hash_key: i32,
    /// The name used for the generated Rust enum type.
    enum_name: String,
    /// The property name as it appears in block state JSON.
    serialized_name: String,
    /// The kind and possible values of this property.
    #[serde(rename = "type")]
    #[serde(flatten)]
    property_type: GeneratedPropertyType,
}

impl GeneratedProperty {
    /// Converts this deserialized property into the intermediate `Property` used during code generation.
    fn to_property(&self) -> Property {
        let enum_name = match &self.property_type {
            GeneratedPropertyType::Boolean => "boolean".to_string(),
            GeneratedPropertyType::Int { min, max } => format!("integer_{min}_to_{max}"),
            GeneratedPropertyType::Enum { .. } => self.enum_name.clone(),
        };

        let values = match &self.property_type {
            GeneratedPropertyType::Boolean => {
                vec!["true".to_string(), "false".to_string()]
            }
            GeneratedPropertyType::Int { min, max } => {
                let mut values = Vec::new();
                for i in *min..=*max {
                    values.push(format!("L{i}"));
                }
                values
            }
            GeneratedPropertyType::Enum { values } => values.clone(),
        };

        Property {
            enum_name,
            serialized_name: self.serialized_name.clone(),
            values,
        }
    }
}

/// Intermediate representation of a block property used while building the property group map.
#[derive(Clone)]
struct Property {
    /// The Rust enum type name for this property.
    enum_name: String,
    /// The JSON key name for this property.
    serialized_name: String,
    /// All possible string values this property can take.
    values: Vec<String>,
}

/// Top-level container for all block assets loaded from `blocks.json`.
#[derive(Deserialize)]
pub struct BlockAssets {
    /// All blocks with their states and properties.
    pub blocks: Vec<Block>,
    /// All unique bounding boxes referenced by block states.
    pub shapes: Vec<BoundingBox>,
    /// All unique sound types referenced by block states.
    pub sound_types: Vec<SoundType>,
    /// Registry names of all block entity types.
    pub block_entity_types: Vec<String>,
}

#[derive(Deserialize)]
struct BlockShapeOffset {
    #[serde(rename = "type")]
    offset_type: BlockShapeOffsetType,
    max_horizontal: f32,
    max_vertical: f32,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BlockShapeOffsetType {
    Xz,
    Xyz,
}

/// Reads all block assets and generates the complete block registry `TokenStream`.
pub fn build() -> TokenStream {
    let be_blocks_data = fs::read("../../assets/bedrock/block_states.nbt").unwrap();
    let mut be_blocks_cursor = Cursor::new(be_blocks_data);
    let be_blocks = get_be_data_from_nbt(&mut be_blocks_cursor);

    let geyser_blocks_data = fs::read("../../assets/bedrock/blocks.nbt").unwrap();
    let geyser_nbt_compound =
        pumpkin_nbt::nbt_compress::read_gzip_compound_tag(Cursor::new(geyser_blocks_data)).unwrap();
    let bedrock_mappings_list = geyser_nbt_compound
        .get_list("bedrock_mappings")
        .expect("Missing bedrock_mappings list in Geyser blocks.nbt");

    let blocks_assets: BlockAssets =
        serde_json::from_str(&fs::read_to_string("../../assets/blocks.json").unwrap())
            .expect("Failed to parse blocks.json");

    let shape_offset_arms = shape_offset_arms(&blocks_assets);

    let generated_properties: Vec<GeneratedProperty> =
        serde_json::from_str(&fs::read_to_string("../../assets/properties.json").unwrap())
            .expect("Failed to parse properties.json");

    let generated_prop_map: BTreeMap<i32, &GeneratedProperty> = generated_properties
        .iter()
        .map(|p| (p.hash_key, p))
        .collect();

    let property_key_names: std::collections::BTreeSet<String> = generated_properties
        .iter()
        .map(|p| p.serialized_name.clone())
        .collect();
    let property_key_variants: Vec<Ident> = property_key_names
        .iter()
        .map(|name| Ident::new(&name.to_upper_camel_case(), Span::call_site()))
        .collect();
    let property_key_literals: Vec<&str> = property_key_names.iter().map(String::as_str).collect();

    let arms = property_value_arms(&generated_properties);

    // Unmapped Java states fall back to Bedrock air.
    let air_be_network_id = *be_blocks
        .get("air")
        .and_then(|variants| variants.first())
        .map(|(id, _)| id)
        .expect("block_states.nbt is missing minecraft:air");

    let mut tables = BlockTables {
        property_enums: property_enum_structs(&generated_properties),
        ..Default::default()
    };
    let ctx = BlockContext {
        generated_prop_map: &generated_prop_map,
        enum_prop_meta: &arms.enum_prop_meta,
        be_blocks: &be_blocks,
        bedrock_mappings_list,
        air_be_network_id,
    };
    for block in blocks_assets.blocks {
        push_block(block, &ctx, &mut tables);
    }
    let BlockTables {
        lists,
        states,
        type_from_raw_id_array,
        property_enums,
        property_collection_map,
        ..
    } = tables;
    let StateLists {
        random_tick_states,
        air_states,
        liquid_states,
        block_state_to_bedrock,
        raw_id_from_state_id_array,
        state_from_state_id_array,
    } = states;
    let PropertyGroupArms {
        block_properties_from_state_and_block_id_arms,
        block_properties_from_props_and_name_arms,
        block_properties,
    } = property_group_arms(property_collection_map);

    let shapes = blocks_assets.shapes.iter().map(ToTokens::to_token_stream);
    let sound_types = blocks_assets
        .sound_types
        .iter()
        .map(ToTokens::to_token_stream);

    let air_state_ids = quote! { #(#air_states)|* };
    let liquid_state_ids = quote! { #(#liquid_states)|* };

    let block_props = block_properties.iter().map(ToTokens::to_token_stream);
    let properties = property_enums.values().map(ToTokens::to_token_stream);

    let block_entity_types = blocks_assets
        .block_entity_types
        .iter()
        .map(|entity_type| LitStr::new(entity_type, Span::call_site()));

    let raw_id_from_state_id_ordered = fill_array(raw_id_from_state_id_array);
    let max_state_id = raw_id_from_state_id_ordered.len();
    let raw_id_from_state_id = quote! { #(#raw_id_from_state_id_ordered),* };

    let max_index = block_state_to_bedrock
        .iter()
        .map(|(idx, _)| *idx)
        .max()
        .unwrap_or(0);
    let mut state_to_bedrock_tokens =
        vec![proc_macro2::Literal::u32_unsuffixed(air_be_network_id); (max_index + 1) as usize];
    for (state_id, id_lit) in block_state_to_bedrock {
        state_to_bedrock_tokens[state_id as usize] = proc_macro2::Literal::u32_unsuffixed(id_lit);
    }
    let block_state_to_bedrock_t = quote! { #(#state_to_bedrock_tokens),* };

    let type_from_raw_id_vec = fill_array(type_from_raw_id_array);
    let max_type_id = type_from_raw_id_vec.len();
    let type_from_raw_id_items = quote! { #(#type_from_raw_id_vec),* };

    let state_from_state_id_vec = fill_state_array(state_from_state_id_array);
    let max_state_id_2 = state_from_state_id_vec.len();
    let state_from_state_id = quote! { #(#state_from_state_id_vec),* };

    assert_eq!(max_state_id, max_state_id_2);

    let Bitset {
        items,
        mod_ident,
        contains_ident,
    } = &gen_u16_bitset(
        "RANDOM_TICKS",
        &random_tick_states
            .iter()
            .map(|it| it.base10_parse().unwrap())
            .collect::<Vec<u16>>(),
    );

    let items_tokens = static_items::items_tokens();
    let block_property_tokens = static_items::block_property_tokens();
    let from_id_tokens = static_items::from_id_tokens();
    let from_registry_key_tokens = static_items::from_registry_key_tokens();
    let facing_tokens = static_items::facing_tokens();
    let rail_shape_tokens = static_items::rail_shape_tokens();

    let property_key = property_key_tokens(&property_key_variants, &property_key_literals);
    let property_enum = property_enum_tokens(&arms);
    let state_tables = state_table_tokens(
        shapes,
        sound_types,
        block_entity_types,
        &air_state_ids,
        &liquid_state_ids,
        mod_ident,
        contains_ident,
    );
    let mappings = mappings_tokens(
        &lists.block_from_name_entries,
        max_state_id,
        &raw_id_from_state_id,
        max_type_id,
        &type_from_raw_id_items,
        &state_from_state_id,
    );
    let block_impl = block_impl_tokens(
        &lists,
        &shape_offset_arms,
        &from_registry_key_tokens,
        &block_properties_from_state_and_block_id_arms,
        &block_properties_from_props_and_name_arms,
    );
    let block_id_impl = block_id_impl_tokens(&lists.block_id_constants);

    quote! {
        #items_tokens

        #items

        #property_key

        #property_enum


        #block_property_tokens

        #state_tables


        impl BlockState {
            const STATE_ID_TO_BEDROCK: &[u32] = &[
                #block_state_to_bedrock_t
            ];

            #from_id_tokens
        }

        impl BlockStateId {
            pub const AIR: Self = Block::AIR.default_state.id;

            pub(crate) const STATE_COUNT: u16 = mappings::STATE_FROM_STATE_ID.len() as u16;
        }

        #mappings

        #block_impl

        #block_id_impl


        #(#properties)*

        #(#block_props)*

        #facing_tokens

        #rail_shape_tokens
    }
}

/// Parses the Bedrock Edition block palette NBT file into a map of block names to their state variants.
///
/// Bedrock network IDs are hashes of the block state, so they are taken verbatim from the dump
/// instead of being derived from the palette order.
///
/// # Arguments
/// – `reader` – a readable byte source positioned at the start of the NBT data.
#[expect(clippy::type_complexity)]
fn get_be_data_from_nbt<R: Read + Seek>(
    reader: &mut R,
) -> BTreeMap<String, Vec<(u32, BTreeMap<String, String>)>> {
    let mut block_data: BTreeMap<String, Vec<(u32, BTreeMap<String, String>)>> = BTreeMap::new();

    let data_start = reader.stream_position().unwrap();
    let data_end = reader.seek(SeekFrom::End(0)).unwrap();
    let _ = reader.seek(SeekFrom::Start(data_start));

    let nbt_reader =
        &mut NbtReadHelperBedrock::new(pumpkin_nbt::deserializer::NbtStreamReader(&mut *reader));

    loop {
        if nbt_reader.reader().0.stream_position().unwrap() >= data_end {
            break;
        }

        let nbt = pumpkin_nbt::Nbt::read(nbt_reader).unwrap();

        let block_name = {
            let raw_name = nbt.get_string("name").unwrap();
            raw_name
                .strip_prefix("minecraft:")
                .unwrap_or(raw_name)
                .to_string()
        };

        let properties = nbt
            .get_compound("states")
            .unwrap()
            .clone()
            .into_iter()
            .map(|(key, val)| {
                let unpacked = match val {
                    pumpkin_nbt::tag::NbtTag::Byte(v) => {
                        if v == 1 {
                            "true".into()
                        } else {
                            "false".into()
                        }
                    }
                    pumpkin_nbt::tag::NbtTag::Int(v) => v.to_string(),
                    pumpkin_nbt::tag::NbtTag::String(v) => v.into(),
                    _ => {
                        panic!("Unexpected type for {}. Value: {val:?}", key);
                    }
                };

                (key.into(), unpacked)
            })
            .collect::<BTreeMap<_, _>>();

        let network_id = nbt
            .get_int("network_id")
            .expect("Missing network_id in block_states.nbt entry") as u32;

        if !block_name.is_empty() {
            block_data
                .entry(block_name)
                .or_default()
                .push((network_id, properties));
        }
    }

    block_data
}

fn parse_geyser_entry(
    entry: &pumpkin_nbt::tag::NbtTag,
    java_block_name: &str,
) -> (String, BTreeMap<String, String>) {
    let compound = match entry {
        pumpkin_nbt::tag::NbtTag::Compound(c) => c,
        _ => panic!("Expected NbtCompound in Geyser bedrock_mappings list"),
    };

    let raw_identifier = compound
        .get_string("bedrock_identifier")
        .filter(|s| !s.is_empty())
        .unwrap_or(java_block_name);

    let bedrock_identifier = raw_identifier
        .strip_prefix("minecraft:")
        .unwrap_or(raw_identifier)
        .to_string();

    let mut properties = BTreeMap::new();
    if let Some(state_comp) = compound.get_compound("state") {
        for (key, val) in state_comp.clone() {
            let unpacked = match val {
                pumpkin_nbt::tag::NbtTag::Byte(v) => {
                    if v == 1 {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }
                pumpkin_nbt::tag::NbtTag::Int(v) => v.to_string(),
                pumpkin_nbt::tag::NbtTag::String(v) => v.to_string(),
                _ => {
                    panic!(
                        "Unexpected NBT type in Geyser state tag for {}. Value: {:?}",
                        key, val
                    );
                }
            };
            properties.insert(key.to_string(), unpacked);
        }
    }

    (bedrock_identifier, properties)
}
