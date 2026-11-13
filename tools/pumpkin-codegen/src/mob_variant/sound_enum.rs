//! The sound-variant enums (cat, cow, pig, chicken, wolf, ..): arms gathered per variant, then the enum and its sound methods.

use super::{SoundGroupJson, SoundVariantJson, sound_ident_from_str};
use heck::ToPascalCase;
use proc_macro2::Ident;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The match arms and variant lists of one sound-variant enum, gathered per variant.
#[derive(Default)]
pub(super) struct SoundArms {
    enum_variants: Vec<TokenStream>,
    from_name_arms: Vec<TokenStream>,
    to_name_arms: Vec<TokenStream>,
    from_id_arms: Vec<TokenStream>,
    ambient_arms: Vec<TokenStream>,
    death_arms: Vec<TokenStream>,
    hurt_arms: Vec<TokenStream>,
    step_arms: Vec<TokenStream>,
    eat_arms: Vec<TokenStream>,
    hiss_arms: Vec<TokenStream>,
    purr_arms: Vec<TokenStream>,
    purreow_arms: Vec<TokenStream>,
    beg_arms: Vec<TokenStream>,
    stray_ambient_arms: Vec<TokenStream>,
    growl_arms: Vec<TokenStream>,
    pant_arms: Vec<TokenStream>,
    whine_arms: Vec<TokenStream>,
    all_variants: Vec<TokenStream>,
}

/// Adds one variant's id, name and sound arms.
pub(super) fn push_variant(
    arms: &mut SoundArms,
    idx: usize,
    name: &String,
    json: &SoundVariantJson,
    has_eat: bool,
    has_cat_specifics: bool,
    has_wolf_specifics: bool,
) {
    let SoundArms {
        enum_variants,
        from_name_arms,
        to_name_arms,
        from_id_arms,
        ..
    } = arms;
    let variant_ident = format_ident!("{}", name.to_pascal_case());
    let namespaced = format!("minecraft:{name}");
    let id_u8 = idx as u8;

    let adult = json.adult_group();
    let baby = json.baby_group();

    if name == "classic" {
        enum_variants.push(quote! {
            #[default]
            #variant_ident = #id_u8
        });
    } else {
        enum_variants.push(quote! {
            #variant_ident = #id_u8
        });
    }

    from_name_arms.push(quote! {
        #namespaced | #name => Some(Self::#variant_ident),
    });
    to_name_arms.push(quote! {
        Self::#variant_ident => #name,
    });
    from_id_arms.push(quote! {
        #id_u8 => Some(Self::#variant_ident),
    });
    push_common_sounds(arms, &adult, &baby, &variant_ident);

    if has_eat {
        let adult_eat = adult.eat_sound.as_deref().map(sound_ident_from_str);
        let baby_eat = baby.eat_sound.as_deref().map(sound_ident_from_str);
        if let (Some(ad), Some(ba)) = (adult_eat, baby_eat) {
            arms.eat_arms.push(quote! {
                Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
            });
        }
    }

    if has_cat_specifics {
        push_cat_sounds(arms, &adult, &baby, &variant_ident);
    }

    if has_wolf_specifics {
        push_wolf_sounds(arms, &adult, &baby, &variant_ident);
    }

    arms.all_variants.push(quote! { Self::#variant_ident });
}

/// The ambient, death, hurt and step sound arms of one variant.
fn push_common_sounds(
    arms: &mut SoundArms,
    adult: &SoundGroupJson,
    baby: &SoundGroupJson,
    variant_ident: &Ident,
) {
    let SoundArms {
        ambient_arms,
        death_arms,
        hurt_arms,
        step_arms,
        ..
    } = arms;
    let adult_amb = adult.ambient_sound.as_deref().map(sound_ident_from_str);
    let baby_amb = baby.ambient_sound.as_deref().map(sound_ident_from_str);
    let adult_death = adult.death_sound.as_deref().map(sound_ident_from_str);
    let baby_death = baby.death_sound.as_deref().map(sound_ident_from_str);
    let adult_hurt = adult.hurt_sound.as_deref().map(sound_ident_from_str);
    let baby_hurt = baby.hurt_sound.as_deref().map(sound_ident_from_str);
    let adult_step = adult.step_sound.as_deref().map(sound_ident_from_str);
    let baby_step = baby.step_sound.as_deref().map(sound_ident_from_str);

    // Ambient sound
    if let (Some(ad), Some(ba)) = (adult_amb, baby_amb) {
        ambient_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    // Death sound
    if let (Some(ad), Some(ba)) = (adult_death, baby_death) {
        death_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    // Hurt sound
    if let (Some(ad), Some(ba)) = (adult_hurt, baby_hurt) {
        hurt_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    // Step sound
    if let (Some(ad), Some(ba)) = (adult_step, baby_step) {
        step_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
}

/// The cat-only sound arms (hiss, purr, purreow, beg, stray ambient) of one variant.
fn push_cat_sounds(
    arms: &mut SoundArms,
    adult: &SoundGroupJson,
    baby: &SoundGroupJson,
    variant_ident: &Ident,
) {
    let SoundArms {
        hiss_arms,
        purr_arms,
        purreow_arms,
        beg_arms,
        stray_ambient_arms,
        ..
    } = arms;
    let adult_hiss = adult.hiss_sound.as_deref().map(sound_ident_from_str);
    let baby_hiss = baby.hiss_sound.as_deref().map(sound_ident_from_str);
    let adult_purr = adult.purr_sound.as_deref().map(sound_ident_from_str);
    let baby_purr = baby.purr_sound.as_deref().map(sound_ident_from_str);
    let adult_purreow = adult.purreow_sound.as_deref().map(sound_ident_from_str);
    let baby_purreow = baby.purreow_sound.as_deref().map(sound_ident_from_str);
    let adult_beg = adult
        .beg_for_food_sound
        .as_deref()
        .map(sound_ident_from_str);
    let baby_beg = baby.beg_for_food_sound.as_deref().map(sound_ident_from_str);
    let stray_amb = adult
        .stray_ambient_sound
        .as_deref()
        .or(adult.ambient_sound.as_deref())
        .map(sound_ident_from_str);

    if let (Some(ad), Some(ba)) = (adult_hiss, baby_hiss) {
        hiss_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    if let (Some(ad), Some(ba)) = (adult_purr, baby_purr) {
        purr_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    if let (Some(ad), Some(ba)) = (adult_purreow, baby_purreow) {
        purreow_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    if let (Some(ad), Some(ba)) = (adult_beg, baby_beg) {
        beg_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    if let Some(stray) = stray_amb {
        stray_ambient_arms.push(quote! {
            Self::#variant_ident => crate::sound::Sound::#stray,
        });
    }
}

/// The wolf-only sound arms (growl, pant, whine) of one variant.
fn push_wolf_sounds(
    arms: &mut SoundArms,
    adult: &SoundGroupJson,
    baby: &SoundGroupJson,
    variant_ident: &Ident,
) {
    let SoundArms {
        growl_arms,
        pant_arms,
        whine_arms,
        ..
    } = arms;
    let adult_growl = adult.growl_sound.as_deref().map(sound_ident_from_str);
    let baby_growl = baby.growl_sound.as_deref().map(sound_ident_from_str);
    let adult_pant = adult.pant_sound.as_deref().map(sound_ident_from_str);
    let baby_pant = baby.pant_sound.as_deref().map(sound_ident_from_str);
    let adult_whine = adult.whine_sound.as_deref().map(sound_ident_from_str);
    let baby_whine = baby.whine_sound.as_deref().map(sound_ident_from_str);

    if let (Some(ad), Some(ba)) = (adult_growl, baby_growl) {
        growl_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    if let (Some(ad), Some(ba)) = (adult_pant, baby_pant) {
        pant_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
    if let (Some(ad), Some(ba)) = (adult_whine, baby_whine) {
        whine_arms.push(quote! {
            Self::#variant_ident => if is_baby { crate::sound::Sound::#ba } else { crate::sound::Sound::#ad },
        });
    }
}

/// The `step_sound` method, when any variant has step sounds.
pub(super) fn step_fn_tokens(arms: &SoundArms) -> TokenStream {
    let SoundArms { step_arms, .. } = arms;
    if !step_arms.is_empty() {
        quote! {
            #[must_use]
            pub const fn step_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #step_arms )*
                }
            }
        }
    } else {
        quote! {}
    }
}

/// The `eat_sound` method of enums that have eat sounds.
pub(super) fn eat_fn_tokens(arms: &SoundArms, has_eat: bool) -> TokenStream {
    let SoundArms { eat_arms, .. } = arms;
    if has_eat {
        quote! {
            #[must_use]
            pub const fn eat_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #eat_arms )*
                }
            }
        }
    } else {
        quote! {}
    }
}

/// The cat-only sound methods.
pub(super) fn cat_fns_tokens(arms: &SoundArms, has_cat_specifics: bool) -> TokenStream {
    let SoundArms {
        hiss_arms,
        purr_arms,
        purreow_arms,
        beg_arms,
        stray_ambient_arms,
        ..
    } = arms;
    if has_cat_specifics {
        quote! {
            #[must_use]
            pub const fn hiss_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #hiss_arms )*
                }
            }

            #[must_use]
            pub const fn purr_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #purr_arms )*
                }
            }

            #[must_use]
            pub const fn purreow_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #purreow_arms )*
                }
            }

            #[must_use]
            pub const fn beg_for_food_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #beg_arms )*
                }
            }

            #[must_use]
            pub const fn stray_ambient_sound(&self) -> crate::sound::Sound {
                match self {
                    #( #stray_ambient_arms )*
                }
            }
        }
    } else {
        quote! {}
    }
}

/// The wolf-only sound methods.
pub(super) fn wolf_fns_tokens(arms: &SoundArms, has_wolf_specifics: bool) -> TokenStream {
    let SoundArms {
        growl_arms,
        pant_arms,
        whine_arms,
        ..
    } = arms;
    if has_wolf_specifics {
        quote! {
            #[must_use]
            pub const fn growl_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #growl_arms )*
                }
            }

            #[must_use]
            pub const fn pant_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #pant_arms )*
                }
            }

            #[must_use]
            pub const fn whine_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #whine_arms )*
                }
            }
        }
    } else {
        quote! {}
    }
}

/// The enum, its lookups and its sound methods.
pub(super) fn sound_enum_tokens(
    enum_ident: &Ident,
    arms: &SoundArms,
    step_fn: &TokenStream,
    eat_fn: &TokenStream,
    cat_fns: &TokenStream,
    wolf_fns: &TokenStream,
) -> TokenStream {
    let SoundArms {
        enum_variants,
        from_name_arms,
        to_name_arms,
        from_id_arms,
        ambient_arms,
        death_arms,
        hurt_arms,
        all_variants,
        ..
    } = arms;
    quote! {
        /* This file is generated. Do not edit manually. */

        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
        #[repr(u8)]
        pub enum #enum_ident {
            #( #enum_variants, )*
        }

        impl #enum_ident {
            pub const ALL: &'static [Self] = &[
                #( #all_variants, )*
            ];

            #[doc = "Returns the sound variant from a resource name (bare or namespaced)."]
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    #( #from_name_arms )*
                    _ => None,
                }
            }

            #[doc = "Returns the numeric ID of the sound variant in the synced registry."]
            #[must_use]
            pub const fn id(&self) -> u8 {
                *self as u8
            }

            #[must_use]
            pub const fn from_id(id: u8) -> Option<Self> {
                match id {
                    #( #from_id_arms )*
                    _ => None,
                }
            }

            #[doc = "Returns the bare string name of the sound variant."]
            #[must_use]
            pub const fn to_name(&self) -> &'static str {
                match self {
                    #( #to_name_arms )*
                }
            }

            #[must_use]
            pub const fn ambient_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #ambient_arms )*
                }
            }

            #[must_use]
            pub const fn death_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #death_arms )*
                }
            }

            #[must_use]
            pub const fn hurt_sound(&self, is_baby: bool) -> crate::sound::Sound {
                match self {
                    #( #hurt_arms )*
                }
            }

            #step_fn
            #eat_fn
            #cat_fns
            #wolf_fns

            #[must_use]
            pub const fn all() -> &'static [Self] {
                Self::ALL
            }
        }
    }
}
