//! Equipping, repairing and enchanting components of an item.

use super::{ItemComponents, StringOrList, entity_idset_tokens};
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
    /// Emits the `equippable` component, if any.
    pub(super) fn push_equippable(&self, tokens: &mut TokenStream) {
        if let Some(equippable) = &self.equippable {
            let slot = match equippable.slot.as_str() {
                "mainhand" => quote! { &EquipmentSlot::MAIN_HAND },
                "offhand" => quote! { &EquipmentSlot::OFF_HAND },
                "head" => quote! { &EquipmentSlot::HEAD },
                "chest" => quote! { &EquipmentSlot::CHEST },
                "legs" => quote! { &EquipmentSlot::LEGS },
                "feet" => quote! { &EquipmentSlot::FEET },
                "body" => quote! { &EquipmentSlot::BODY },
                "saddle" => quote! { &EquipmentSlot::SADDLE },
                _ => panic!("Unknown equippable slot: {}", equippable.slot),
            };
            let equip_sound = equippable
                .equip_sound
                .as_ref()
                .map(|s| {
                    let variant_name =
                        format_ident!("{}", s.strip_prefix("minecraft:").unwrap().to_pascal_case());
                    quote! { IdOr::Id(Sound::#variant_name) }
                })
                .unwrap_or(quote! { IdOr::Id(Sound::ItemArmorEquipGeneric) });
            let asset_id = equippable
                .asset_id
                .as_ref()
                .map(|s| {
                    let asset_id = LitStr::new(s, Span::call_site());
                    quote! { Some(Cow::Borrowed(#asset_id)) }
                })
                .unwrap_or(quote! { None });
            let camera_overlay = equippable
                .camera_overlay
                .as_ref()
                .map(|s| {
                    let camera_overlay = LitStr::new(s, Span::call_site());
                    quote! { Some(Cow::Borrowed(#camera_overlay)) }
                })
                .unwrap_or(quote! { None });
            let entities_option = equippable.allowed_entities.as_ref().map_or_else(
                || quote! { None },
                |entities| {
                    let allowed_entities = entity_idset_tokens(entities);
                    quote! { Some(#allowed_entities) }
                },
            );
            let dispensable = LitBool::new(equippable.dispensable, Span::call_site());
            let swappable = LitBool::new(equippable.swappable, Span::call_site());
            let damage_on_hurt = LitBool::new(equippable.damage_on_hurt, Span::call_site());
            let equip_on_interact = LitBool::new(equippable.equip_on_interact, Span::call_site());
            let can_be_sheared = LitBool::new(equippable.can_be_sheared, Span::call_site());
            let shearing_sound = equippable
                .shearing_sound
                .as_ref()
                .map(|s| {
                    let variant_name =
                        format_ident!("{}", s.strip_prefix("minecraft:").unwrap().to_pascal_case());
                    quote! { IdOr::Id(Sound::#variant_name) }
                })
                .unwrap_or(quote! { IdOr::Id(Sound::ItemShearsSnip) });

            tokens.extend(quote! { (Equippable, &EquippableImpl {
                slot: #slot,
                equip_sound: #equip_sound,
                asset_id: #asset_id,
                camera_overlay: #camera_overlay,
                allowed_entities: #entities_option,
                dispensable: #dispensable,
                swappable: #swappable,
                damage_on_hurt: #damage_on_hurt,
                equip_on_interact: #equip_on_interact,
                can_be_sheared: #can_be_sheared,
                shearing_sound: #shearing_sound
            }), });
        }
    }

    /// Emits the `enchantable` component, if any.
    pub(super) fn push_enchantable(&self, tokens: &mut TokenStream) {
        if let Some(enchantable) = &self.enchantable {
            let value = LitInt::new(&enchantable.value.to_string(), Span::call_site());
            tokens.extend(quote! { (Enchantable, &EnchantableImpl { value: #value }), });
        }
    }

    /// Emits the `enchantment_glint_override` component, if any.
    pub(super) fn push_enchantment_glint_override(&self, tokens: &mut TokenStream) {
        if let Some(glint) = self.enchantment_glint_override {
            let glint_lit = LitBool::new(glint, Span::call_site());
            tokens.extend(quote! {
                (EnchantmentGlintOverride, &EnchantmentGlintOverrideImpl { glint: #glint_lit }),
            });
        }
    }

    /// Emits the `enchantments` component, if any.
    pub(super) fn push_enchantments(&self, tokens: &mut TokenStream) {
        if self.enchantments.is_some() {
            tokens.extend(
                quote! { (Enchantments, &EnchantmentsImpl { enchantment: Cow::Borrowed(&[]) }), },
            );
        }
    }

    /// Emits the `provides_trim_material` component, if any.
    pub(super) fn push_provides_trim_material(&self, tokens: &mut TokenStream) {
        if let Some(material) = &self.provides_trim_material {
            let mat_lit = LitStr::new(material, Span::call_site());
            tokens.extend(quote! {
                (ProvidesTrimMaterial, &ProvidesTrimMaterialImpl {
                    material: Cow::Borrowed(#mat_lit),
                }),
            });
        }
    }

    /// Emits the `repairable` component, if any.
    pub(super) fn push_repairable(&self, tokens: &mut TokenStream) {
        if let Some(repairable) = &self.repairable {
            let mut items_tokens = TokenStream::new();
            match &repairable.items {
                StringOrList::String(str) => {
                    if let Some(formatted) = str.strip_prefix('#') {
                        items_tokens.extend(quote! {
                            IDSet::Tag(Cow::Borrowed(#formatted))
                        });
                    } else {
                        let item_name = str.strip_prefix("minecraft:").unwrap_or(str);
                        let ident = format_ident!("{}", item_name.to_shouty_snake_case());
                        items_tokens.extend(quote! {
                            IDSet::IDs(Cow::Borrowed(&[&Self::#ident]))
                        });
                    }
                }
                StringOrList::List(items) => {
                    let mut ids = TokenStream::new();
                    for x in items {
                        let item_name = x.strip_prefix("minecraft:").unwrap_or(x);
                        let ident = format_ident!("{}", item_name.to_shouty_snake_case());
                        ids.extend(quote! { &Self::#ident, });
                    }
                    items_tokens.extend(quote! {
                        IDSet::IDs(Cow::Borrowed(&[#ids]))
                    });
                }
            }
            tokens.extend(quote! { (Repairable, &RepairableImpl { items: #items_tokens }), });
        }
    }
}
