//! Stack size, name, look, sound and the other plain components of an item.

use super::{ItemComponents, entity_idset_tokens, float_literal};
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

impl ItemComponents {
    /// Emits the `max_stack_size` component.
    pub(super) fn push_max_stack_size(&self, tokens: &mut TokenStream) {
        let max_stack_size = LitInt::new(&self.max_stack_size.to_string(), Span::call_site());
        tokens.extend(quote! {
            (MaxStackSize, &MaxStackSizeImpl {
                size: #max_stack_size,
            }),
        });
    }

    /// Emits the `jukebox_playable` component, if any.
    pub(super) fn push_jukebox_playable(&self, tokens: &mut TokenStream) {
        if let Some(playable) = &self.jukebox_playable {
            let song = LitStr::new(playable, Span::call_site());
            tokens.extend(quote! {
                (JukeboxPlayable, &JukeboxPlayableImpl{
                    song: #song,
                }),
            });
        }
    }

    /// Emits the `item_name` component.
    pub(super) fn push_item_name(&self, tokens: &mut TokenStream) {
        let TextContent::Translate {
            translate: text,
            bedrock_translate: _,
            with: _,
        } = *self.item_name.clone().0.content
        else {
            unreachable!()
        };
        let item_name = LitStr::new(&text, Span::call_site());
        tokens.extend(quote! {
            (ItemName, &ItemNameImpl {
                name: Cow::Borrowed(#item_name),
            }),
        });
    }

    /// Emits the `block_state` component, if any.
    pub(super) fn push_block_state(&self, tokens: &mut TokenStream) {
        if let Some(block_state) = &self.block_state {
            let mut entries = TokenStream::new();
            if let serde_json::Value::Object(map) = block_state {
                for (k, v) in map {
                    let v_str = match v {
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    };
                    let k_lit = LitStr::new(k, Span::call_site());
                    let v_lit = LitStr::new(&v_str, Span::call_site());
                    entries.extend(quote! {
                        (Cow::Borrowed(#k_lit), Cow::Borrowed(#v_lit)),
                    });
                }
            }
            tokens.extend(quote! {
                (BlockState, &BlockStateImpl {
                    properties: Cow::Borrowed(&[#entries])
                }),
            });
        }
    }

    /// Emits the `block_transformer` component, if any.
    pub(super) fn push_block_transformer(&self, tokens: &mut TokenStream) {
        if let Some(transformer) = &self.block_transformer {
            let tr_lit = LitStr::new(transformer, Span::call_site());
            tokens.extend(quote! {
                (BlockTransformer, &BlockTransformerImpl {
                    transformer: Cow::Borrowed(#tr_lit),
                }),
            });
        }
    }

    /// Emits the `break_sound` component, if any.
    pub(super) fn push_break_sound(&self, tokens: &mut TokenStream) {
        if let Some(break_sound) = &self.break_sound {
            let variant = format_ident!(
                "{}",
                break_sound
                    .strip_prefix("minecraft:")
                    .unwrap_or(break_sound)
                    .to_pascal_case()
            );
            tokens.extend(quote! {
                (BreakSound, &BreakSoundImpl {
                    sound: IdOr::Id(Sound::#variant),
                }),
            });
        }
    }

    /// Emits the `chicken_variant` component, if any.
    pub(super) fn push_chicken_variant(&self, tokens: &mut TokenStream) {
        if let Some(val) = &self.chicken_variant {
            let val_str = match val {
                serde_json::Value::String(s) => s.clone(),
                _ => val.to_string(),
            };
            let val_lit = LitStr::new(&val_str, Span::call_site());
            tokens.extend(quote! { (ChickenVariant, &ChickenVariantImpl { value: Cow::Borrowed(#val_lit) }), });
        }
    }

    /// Emits the `cushion_color` component, if any.
    pub(super) fn push_cushion_color(&self, tokens: &mut TokenStream) {
        if let Some(color) = &self.cushion_color {
            let color_ident = format_ident!("{}", color.to_pascal_case());
            tokens.extend(quote! {
                (CushionColor, &CushionColorImpl {
                    color: crate::dye_color::DyeColor::#color_ident,
                }),
            });
        }
    }

    /// Emits the `dye` component, if any.
    pub(super) fn push_dye(&self, tokens: &mut TokenStream) {
        if let Some(dye) = &self.dye {
            let color_ident = format_ident!("{}", dye.to_pascal_case());
            tokens.extend(quote! {
                (Dye, &DyeImpl {
                    color: crate::dye_color::DyeColor::#color_ident,
                }),
            });
        }
    }

    /// Emits the `fireworks` component, if any.
    pub(super) fn push_fireworks(&self, tokens: &mut TokenStream) {
        if let Some(fireworks) = &self.fireworks {
            let flight_duration = if let serde_json::Value::Object(map) = fireworks
                && let Some(serde_json::Value::Number(num)) = map.get("flight_duration")
            {
                num.as_i64().unwrap_or(1) as i32
            } else {
                1
            };
            tokens.extend(quote! {
                (Fireworks, &FireworksImpl {
                    flight_duration: #flight_duration,
                    explosions: Vec::new(),
                }),
            });
        }
    }

    /// Emits the `instrument` component, if any.
    pub(super) fn push_instrument(&self, tokens: &mut TokenStream) {
        if let Some(instrument) = &self.instrument {
            let inst_lit = LitStr::new(instrument, Span::call_site());
            tokens.extend(quote! {
                (Instrument, &InstrumentImpl {
                    instrument: Cow::Borrowed(#inst_lit),
                }),
            });
        }
    }

    /// Emits the `item_model` component, if any.
    pub(super) fn push_item_model(&self, tokens: &mut TokenStream) {
        if let Some(model) = &self.item_model {
            let model_lit = LitStr::new(model, Span::call_site());
            tokens
                .extend(quote! { (ItemModel, &ItemModelImpl { id: Cow::Borrowed(#model_lit) }), });
        }
    }

    /// Emits the `mob_visibility` component, if any.
    pub(super) fn push_mob_visibility(&self, tokens: &mut TokenStream) {
        if let Some(mob_vis) = &self.mob_visibility {
            let targeting_tokens = entity_idset_tokens(&mob_vis.targeting_entity_types);
            let visibility = float_literal(mob_vis.visibility);
            tokens.extend(quote! {
                (MobVisibility, &MobVisibilityImpl {
                    targeting_entity_types: #targeting_tokens,
                    visibility: #visibility,
                }),
            });
        }
    }

    /// Emits the `provides_banner_patterns` component, if any.
    pub(super) fn push_provides_banner_patterns(&self, tokens: &mut TokenStream) {
        if let Some(key) = &self.provides_banner_patterns {
            let key_lit = LitStr::new(key, Span::call_site());
            tokens.extend(quote! {
                (ProvidesBannerPatterns, &ProvidesBannerPatternsImpl {
                    key: Cow::Borrowed(#key_lit),
                }),
            });
        }
    }

    /// Emits the `provides_pottery_pattern` component, if any.
    pub(super) fn push_provides_pottery_pattern(&self, tokens: &mut TokenStream) {
        if let Some(pattern) = &self.provides_pottery_pattern {
            let pat_lit = LitStr::new(pattern, Span::call_site());
            tokens.extend(quote! {
                (ProvidesPotteryPattern, &ProvidesPotteryPatternImpl {
                    pattern: Cow::Borrowed(#pat_lit),
                }),
            });
        }
    }

    /// Emits the `rarity` component, if any.
    pub(super) fn push_rarity(&self, tokens: &mut TokenStream) {
        if let Some(rarity_str) = &self.rarity {
            let rarity_variant = match rarity_str.as_str() {
                "uncommon" => quote! { crate::data_component_impl::Rarity::Uncommon },
                "rare" => quote! { crate::data_component_impl::Rarity::Rare },
                "epic" => quote! { crate::data_component_impl::Rarity::Epic },
                _ => quote! { crate::data_component_impl::Rarity::Common },
            };
            tokens.extend(quote! { (Rarity, &RarityImpl { rarity: #rarity_variant }), });
        }
    }

    /// Emits the `writable_book_content` component, if any.
    pub(super) fn push_writable_book_content(&self, tokens: &mut TokenStream) {
        if self.writable_book_content.is_some() {
            tokens.extend(
                quote! { (WritableBookContent, &WritableBookContentImpl { pages: Vec::new() }), },
            );
        }
    }
}
