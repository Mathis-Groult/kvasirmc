//! The fixed part of the generated tag file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn get_tag_values_tokens() -> TokenStream {
    quote! {
        #[must_use]
        pub fn get_tag_values(tag_category: RegistryKey, tag: &str) -> Option<&'static [&'static str]> {
            get_latest_map(tag_category).get(tag).map(|t| t.0)
        }

        #[must_use]
        pub fn get_tag_ids(tag_category: RegistryKey, tag: &str) -> Option<&'static [u16]> {
            get_latest_map(tag_category).get(tag).map(|t| t.1)
        }

        #[must_use]
        pub const fn get_registry_key_tags(version: JavaMinecraftVersion, tag_category: RegistryKey) -> Option<&'static phf::Map<&'static str, &'static Tag>> {
            if !tag_category.is_valid_for_version(version) {
                return None;
            }
            Some(get_latest_map(tag_category))
        }

        pub trait Taggable {
            fn tag_key() -> RegistryKey;
            fn registry_key(&self) -> &str;
            fn registry_id(&self) -> u16;

            #[must_use]
            fn is_tagged_with(&self, tag: &str) -> Option<bool> {
                let tag = tag.strip_prefix('#').unwrap_or(tag);
                let items = get_tag_ids(Self::tag_key(), tag).or_else(|| {
                    if !tag.contains(':') {
                        let mut full = String::with_capacity(10 + tag.len());
                        full.push_str("minecraft:");
                        full.push_str(tag);
                        get_tag_ids(Self::tag_key(), &full)
                    } else if let Some(stripped) = tag.strip_prefix("minecraft:") {
                        get_tag_ids(Self::tag_key(), stripped)
                    } else {
                        None
                    }
                })?;
                Some(items.contains(&self.registry_id()))
            }

            #[must_use]
            fn has_tag(&self, tag: &'static Tag) -> bool {
                tag.1.contains(&self.registry_id())
            }

            #[must_use]
            fn get_tag_values(tag: &str) -> Option<&'static [&'static str]> {
                let tag = tag.strip_prefix('#').unwrap_or(tag);
                get_tag_values(Self::tag_key(), tag).or_else(|| {
                    if !tag.contains(':') {
                        let mut full = String::with_capacity(10 + tag.len());
                        full.push_str("minecraft:");
                        full.push_str(tag);
                        get_tag_values(Self::tag_key(), &full)
                    } else if let Some(stripped) = tag.strip_prefix("minecraft:") {
                        get_tag_values(Self::tag_key(), stripped)
                    } else {
                        None
                    }
                })
            }
        }
    }
}
