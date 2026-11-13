//! The Bedrock-only player calls a plugin can make (`bedrock-player`), with their conversions.

use super::disconnect_reason::from_wasm_bedrock_disconnect_reason;
use crate::forms::FormsHostStateExt;
use crate::pumpkin::plugin::forms::Form;
use crate::{
    pumpkin::{self, plugin::uuid::Uuid},
    uuid::UuidExt,
};
use pumpkin_core::entity::EntityBase;
use pumpkin_protocol::bedrock::client::PackIdVersion;
use pumpkin_protocol::bedrock::client::modal_form_request::CModalFormRequest;
use pumpkin_protocol::bedrock::client::set_actor_data::{
    CSetActorData, MetadataValue, PropertySyncData, SyncedActorDataList, entity_data_key,
};
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_util::translation::Locale;
use pumpkin_util::version::BedrockMinecraftVersion;
use pumpkin_wasm_host_common::state::PluginHostState;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use wasmtime::component::{Access, HasSelf, Resource};

const fn to_wasm_bedrock_version(
    version: BedrockMinecraftVersion,
) -> pumpkin::plugin::player::BedrockMinecraftVersion {
    match version {
        BedrockMinecraftVersion::V_1_21 => pumpkin::plugin::player::BedrockMinecraftVersion::V121,
        BedrockMinecraftVersion::V_1_26_51 | BedrockMinecraftVersion::V_1_26_60 => {
            // The v0.1 ABI does not define this Bedrock version.
            pumpkin::plugin::player::BedrockMinecraftVersion::Unknown
        }
        BedrockMinecraftVersion::Unknown => {
            pumpkin::plugin::player::BedrockMinecraftVersion::Unknown
        }
    }
}

const fn to_wasm_bedrock_device_os(os: i32) -> pumpkin::plugin::player::BedrockDeviceOs {
    match os {
        1 => pumpkin::plugin::player::BedrockDeviceOs::Android,
        2 => pumpkin::plugin::player::BedrockDeviceOs::Ios,
        3 => pumpkin::plugin::player::BedrockDeviceOs::Osx,
        4 => pumpkin::plugin::player::BedrockDeviceOs::Amazon,
        5 => pumpkin::plugin::player::BedrockDeviceOs::GearVr,
        6 => pumpkin::plugin::player::BedrockDeviceOs::HoloLens,
        7 => pumpkin::plugin::player::BedrockDeviceOs::Windows10,
        8 => pumpkin::plugin::player::BedrockDeviceOs::Win32,
        9 => pumpkin::plugin::player::BedrockDeviceOs::Dedicated,
        10 => pumpkin::plugin::player::BedrockDeviceOs::TvOs,
        11 => pumpkin::plugin::player::BedrockDeviceOs::Playstation,
        12 => pumpkin::plugin::player::BedrockDeviceOs::Nintendo,
        13 => pumpkin::plugin::player::BedrockDeviceOs::Xbox,
        14 => pumpkin::plugin::player::BedrockDeviceOs::WindowsPhone,
        15 => pumpkin::plugin::player::BedrockDeviceOs::Linux,
        _ => pumpkin::plugin::player::BedrockDeviceOs::Unknown,
    }
}

const fn to_wasm_bedrock_input_mode(mode: i32) -> pumpkin::plugin::player::BedrockInputMode {
    match mode {
        1 => pumpkin::plugin::player::BedrockInputMode::Mouse,
        2 => pumpkin::plugin::player::BedrockInputMode::Touch,
        3 => pumpkin::plugin::player::BedrockInputMode::GamePad,
        4 => pumpkin::plugin::player::BedrockInputMode::MotionController,
        _ => pumpkin::plugin::player::BedrockInputMode::Unknown,
    }
}

const fn to_wasm_bedrock_ui_profile(profile: i32) -> pumpkin::plugin::player::BedrockUiProfile {
    match profile {
        0 => pumpkin::plugin::player::BedrockUiProfile::Classic,
        1 => pumpkin::plugin::player::BedrockUiProfile::Pocket,
        _ => pumpkin::plugin::player::BedrockUiProfile::Unknown,
    }
}

const fn to_wasm_bedrock_graphics_mode(mode: i32) -> pumpkin::plugin::player::BedrockGraphicsMode {
    match mode {
        0 => pumpkin::plugin::player::BedrockGraphicsMode::Simple,
        1 => pumpkin::plugin::player::BedrockGraphicsMode::Fancy,
        2 => pumpkin::plugin::player::BedrockGraphicsMode::RayTraced,
        _ => pumpkin::plugin::player::BedrockGraphicsMode::Unknown,
    }
}

const fn from_wasm_bedrock_ability(
    ability: pumpkin::plugin::player::BedrockAbility,
) -> pumpkin_protocol::bedrock::client::update_abilities::Ability {
    match ability {
        pumpkin::plugin::player::BedrockAbility::Build => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Build
        }
        pumpkin::plugin::player::BedrockAbility::Mine => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Mine
        }
        pumpkin::plugin::player::BedrockAbility::DoorsAndSwitches => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::DoorsAndSwitches
        }
        pumpkin::plugin::player::BedrockAbility::OpenContainers => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::OpenContainers
        }
        pumpkin::plugin::player::BedrockAbility::AttackPlayers => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::AttackPlayers
        }
        pumpkin::plugin::player::BedrockAbility::AttackMobs => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::AttackMobs
        }
        pumpkin::plugin::player::BedrockAbility::OperatorCommands => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::OperatorCommands
        }
        pumpkin::plugin::player::BedrockAbility::Teleport => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Teleport
        }
        pumpkin::plugin::player::BedrockAbility::Invulnerable => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Invulnerable
        }
        pumpkin::plugin::player::BedrockAbility::Flying => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Flying
        }
        pumpkin::plugin::player::BedrockAbility::MayFly => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::MayFly
        }
        pumpkin::plugin::player::BedrockAbility::Instabuild => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Instabuild
        }
        pumpkin::plugin::player::BedrockAbility::Lightning => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Lightning
        }
        pumpkin::plugin::player::BedrockAbility::FlySpeed => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::FlySpeed
        }
        pumpkin::plugin::player::BedrockAbility::WalkSpeed => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::WalkSpeed
        }
        pumpkin::plugin::player::BedrockAbility::Muted => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Muted
        }
        pumpkin::plugin::player::BedrockAbility::WorldBuilder => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::WorldBuilder
        }
        pumpkin::plugin::player::BedrockAbility::NoClip => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::NoClip
        }
        pumpkin::plugin::player::BedrockAbility::PrivilegedBuilder => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::PrivilegedBuilder
        }
        pumpkin::plugin::player::BedrockAbility::VerticalFlySpeed => {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::VerticalFlySpeed
        }
    }
}

#[allow(clippy::too_many_lines)]
const fn from_wasm_bedrock_status_flag(flag: pumpkin::plugin::player::BedrockStatusFlag) -> u32 {
    use pumpkin_protocol::bedrock::client::set_actor_data::entity_data_flag;
    match flag {
        pumpkin::plugin::player::BedrockStatusFlag::OnFire => entity_data_flag::ON_FIRE,
        pumpkin::plugin::player::BedrockStatusFlag::Sneaking => entity_data_flag::SNEAKING,
        pumpkin::plugin::player::BedrockStatusFlag::Riding => entity_data_flag::RIDING,
        pumpkin::plugin::player::BedrockStatusFlag::Sprinting => entity_data_flag::SPRINTING,
        pumpkin::plugin::player::BedrockStatusFlag::UsingItem => entity_data_flag::USING_ITEM,
        pumpkin::plugin::player::BedrockStatusFlag::Invisible => entity_data_flag::INVISIBLE,
        pumpkin::plugin::player::BedrockStatusFlag::Tempted => entity_data_flag::TEMPTED,
        pumpkin::plugin::player::BedrockStatusFlag::InLove => entity_data_flag::IN_LOVE,
        pumpkin::plugin::player::BedrockStatusFlag::Saddled => entity_data_flag::SADDLED,
        pumpkin::plugin::player::BedrockStatusFlag::Powered => entity_data_flag::POWERED,
        pumpkin::plugin::player::BedrockStatusFlag::Ignited => entity_data_flag::IGNITED,
        pumpkin::plugin::player::BedrockStatusFlag::Baby => entity_data_flag::BABY,
        pumpkin::plugin::player::BedrockStatusFlag::Converting => entity_data_flag::CONVERTING,
        pumpkin::plugin::player::BedrockStatusFlag::Critical => entity_data_flag::CRITICAL,
        pumpkin::plugin::player::BedrockStatusFlag::ShowName => entity_data_flag::SHOW_NAME,
        pumpkin::plugin::player::BedrockStatusFlag::AlwaysShowName => {
            entity_data_flag::ALWAYS_SHOW_NAME
        }
        pumpkin::plugin::player::BedrockStatusFlag::NoAi => entity_data_flag::NO_AI,
        pumpkin::plugin::player::BedrockStatusFlag::Silent => entity_data_flag::SILENT,
        pumpkin::plugin::player::BedrockStatusFlag::WallClimbing => entity_data_flag::WALL_CLIMBING,
        pumpkin::plugin::player::BedrockStatusFlag::Climb => entity_data_flag::CLIMB,
        pumpkin::plugin::player::BedrockStatusFlag::Swim => entity_data_flag::SWIM,
        pumpkin::plugin::player::BedrockStatusFlag::Fly => entity_data_flag::FLY,
        pumpkin::plugin::player::BedrockStatusFlag::Walk => entity_data_flag::WALK,
        pumpkin::plugin::player::BedrockStatusFlag::Resting => entity_data_flag::RESTING,
        pumpkin::plugin::player::BedrockStatusFlag::Sitting => entity_data_flag::SITTING,
        pumpkin::plugin::player::BedrockStatusFlag::Angry => entity_data_flag::ANGRY,
        pumpkin::plugin::player::BedrockStatusFlag::Interested => entity_data_flag::INTERESTED,
        pumpkin::plugin::player::BedrockStatusFlag::Charged => entity_data_flag::CHARGED,
        pumpkin::plugin::player::BedrockStatusFlag::Tamed => entity_data_flag::TAMED,
        pumpkin::plugin::player::BedrockStatusFlag::Orphaned => entity_data_flag::ORPHANED,
        pumpkin::plugin::player::BedrockStatusFlag::Leashed => entity_data_flag::LEASHED,
        pumpkin::plugin::player::BedrockStatusFlag::Sheared => entity_data_flag::SHEARED,
        pumpkin::plugin::player::BedrockStatusFlag::Gliding => entity_data_flag::GLIDING,
        pumpkin::plugin::player::BedrockStatusFlag::Elder => entity_data_flag::ELDER,
        pumpkin::plugin::player::BedrockStatusFlag::Moving => entity_data_flag::MOVING,
        pumpkin::plugin::player::BedrockStatusFlag::Breathing => entity_data_flag::BREATHING,
        pumpkin::plugin::player::BedrockStatusFlag::Chested => entity_data_flag::CHESTED,
        pumpkin::plugin::player::BedrockStatusFlag::Stackable => entity_data_flag::STACKABLE,
        pumpkin::plugin::player::BedrockStatusFlag::ShowBottom => entity_data_flag::SHOW_BOTTOM,
        pumpkin::plugin::player::BedrockStatusFlag::Standing => entity_data_flag::STANDING,
        pumpkin::plugin::player::BedrockStatusFlag::Shaking => entity_data_flag::SHAKING,
        pumpkin::plugin::player::BedrockStatusFlag::Idling => entity_data_flag::IDLING,
        pumpkin::plugin::player::BedrockStatusFlag::Casting => entity_data_flag::CASTING,
        pumpkin::plugin::player::BedrockStatusFlag::Charging => entity_data_flag::CHARGING,
        pumpkin::plugin::player::BedrockStatusFlag::KeyboardControlled => {
            entity_data_flag::KEYBOARD_CONTROLLED
        }
        pumpkin::plugin::player::BedrockStatusFlag::PowerJump => entity_data_flag::POWER_JUMP,
        pumpkin::plugin::player::BedrockStatusFlag::Dash => entity_data_flag::DASH,
        pumpkin::plugin::player::BedrockStatusFlag::Lingering => entity_data_flag::LINGERING,
        pumpkin::plugin::player::BedrockStatusFlag::HasCollision => entity_data_flag::HAS_COLLISION,
        pumpkin::plugin::player::BedrockStatusFlag::HasGravity => entity_data_flag::HAS_GRAVITY,
        pumpkin::plugin::player::BedrockStatusFlag::FireImmune => entity_data_flag::FIRE_IMMUNE,
        pumpkin::plugin::player::BedrockStatusFlag::Dancing => entity_data_flag::DANCING,
        pumpkin::plugin::player::BedrockStatusFlag::Enchanted => entity_data_flag::ENCHANTED,
        pumpkin::plugin::player::BedrockStatusFlag::ReturnTrident => {
            entity_data_flag::RETURN_TRIDENT
        }
        pumpkin::plugin::player::BedrockStatusFlag::ContainerPrivate => {
            entity_data_flag::CONTAINER_PRIVATE
        }
        pumpkin::plugin::player::BedrockStatusFlag::Transforming => entity_data_flag::TRANSFORMING,
        pumpkin::plugin::player::BedrockStatusFlag::DamageNearbyMobs => {
            entity_data_flag::DAMAGE_NEARBY_MOBS
        }
        pumpkin::plugin::player::BedrockStatusFlag::Swimming => entity_data_flag::SWIMMING,
        pumpkin::plugin::player::BedrockStatusFlag::Bribed => entity_data_flag::BRIBED,
        pumpkin::plugin::player::BedrockStatusFlag::Pregnant => entity_data_flag::PREGNANT,
        pumpkin::plugin::player::BedrockStatusFlag::LayingEgg => entity_data_flag::LAYING_EGG,
        pumpkin::plugin::player::BedrockStatusFlag::PassengerCanPick => {
            entity_data_flag::PASSENGER_CAN_PICK
        }
        pumpkin::plugin::player::BedrockStatusFlag::TransitionSitting => {
            entity_data_flag::TRANSITION_SITTING
        }
        pumpkin::plugin::player::BedrockStatusFlag::Eating => entity_data_flag::EATING,
        pumpkin::plugin::player::BedrockStatusFlag::LayingDown => entity_data_flag::LAYING_DOWN,
        pumpkin::plugin::player::BedrockStatusFlag::Sneezing => entity_data_flag::SNEEZING,
        pumpkin::plugin::player::BedrockStatusFlag::Trusting => entity_data_flag::TRUSTING,
        pumpkin::plugin::player::BedrockStatusFlag::Rolling => entity_data_flag::ROLLING,
        pumpkin::plugin::player::BedrockStatusFlag::Scared => entity_data_flag::SCARED,
        pumpkin::plugin::player::BedrockStatusFlag::InScaffolding => {
            entity_data_flag::IN_SCAFFOLDING
        }
        pumpkin::plugin::player::BedrockStatusFlag::OverScaffolding => {
            entity_data_flag::OVER_SCAFFOLDING
        }
        pumpkin::plugin::player::BedrockStatusFlag::DescendThroughBlock => {
            entity_data_flag::DESCEND_THROUGH_BLOCK
        }
        pumpkin::plugin::player::BedrockStatusFlag::Blocking => entity_data_flag::BLOCKING,
        pumpkin::plugin::player::BedrockStatusFlag::TransitionBlocking => {
            entity_data_flag::TRANSITION_BLOCKING
        }
        pumpkin::plugin::player::BedrockStatusFlag::BlockedUsingShield => {
            entity_data_flag::BLOCKED_USING_SHIELD
        }
        pumpkin::plugin::player::BedrockStatusFlag::BlockedUsingDamagedShield => {
            entity_data_flag::BLOCKED_USING_DAMAGED_SHIELD
        }
        pumpkin::plugin::player::BedrockStatusFlag::Sleeping => entity_data_flag::SLEEPING,
        pumpkin::plugin::player::BedrockStatusFlag::WantsToWake => entity_data_flag::WANTS_TO_WAKE,
        pumpkin::plugin::player::BedrockStatusFlag::TradeInterest => {
            entity_data_flag::TRADE_INTEREST
        }
        pumpkin::plugin::player::BedrockStatusFlag::DoorBreaker => entity_data_flag::DOOR_BREAKER,
        pumpkin::plugin::player::BedrockStatusFlag::BreakingObstruction => {
            entity_data_flag::BREAKING_OBSTRUCTION
        }
        pumpkin::plugin::player::BedrockStatusFlag::DoorOpener => entity_data_flag::DOOR_OPENER,
        pumpkin::plugin::player::BedrockStatusFlag::Captain => entity_data_flag::CAPTAIN,
        pumpkin::plugin::player::BedrockStatusFlag::Stunned => entity_data_flag::STUNNED,
        pumpkin::plugin::player::BedrockStatusFlag::Roaring => entity_data_flag::ROARING,
        pumpkin::plugin::player::BedrockStatusFlag::DelayedAttack => {
            entity_data_flag::DELAYED_ATTACK
        }
        pumpkin::plugin::player::BedrockStatusFlag::AvoidingMobs => entity_data_flag::AVOIDING_MOBS,
        pumpkin::plugin::player::BedrockStatusFlag::AvoidingBlock => {
            entity_data_flag::AVOIDING_BLOCK
        }
        pumpkin::plugin::player::BedrockStatusFlag::FacingTargetToRangeAttack => {
            entity_data_flag::FACING_TARGET_TO_RANGE_ATTACK
        }
        pumpkin::plugin::player::BedrockStatusFlag::HiddenWhenInvisible => {
            entity_data_flag::HIDDEN_WHEN_INVISIBLE
        }
        pumpkin::plugin::player::BedrockStatusFlag::InUi => entity_data_flag::IN_UI,
        pumpkin::plugin::player::BedrockStatusFlag::Stalking => entity_data_flag::STALKING,
        pumpkin::plugin::player::BedrockStatusFlag::Emoting => entity_data_flag::EMOTING,
        pumpkin::plugin::player::BedrockStatusFlag::Celebrating => entity_data_flag::CELEBRATING,
        pumpkin::plugin::player::BedrockStatusFlag::Admiring => entity_data_flag::ADMIRING,
        pumpkin::plugin::player::BedrockStatusFlag::CelebratingSpecial => {
            entity_data_flag::CELEBRATING_SPECIAL
        }
        pumpkin::plugin::player::BedrockStatusFlag::OutOfControl => {
            entity_data_flag::OUT_OF_CONTROL
        }
        pumpkin::plugin::player::BedrockStatusFlag::RamAttack => entity_data_flag::RAM_ATTACK,
        pumpkin::plugin::player::BedrockStatusFlag::PlayingDead => entity_data_flag::PLAYING_DEAD,
        pumpkin::plugin::player::BedrockStatusFlag::InAscendingBlock => {
            entity_data_flag::IN_ASCENDING_BLOCK
        }
        pumpkin::plugin::player::BedrockStatusFlag::OverDescendingBlock => {
            entity_data_flag::OVER_DESCENDING_BLOCK
        }
        pumpkin::plugin::player::BedrockStatusFlag::Croaking => entity_data_flag::CROAKING,
        pumpkin::plugin::player::BedrockStatusFlag::DigestMob => entity_data_flag::DIGEST_MOB,
        pumpkin::plugin::player::BedrockStatusFlag::JumpGoal => entity_data_flag::JUMP_GOAL,
        pumpkin::plugin::player::BedrockStatusFlag::Emerging => entity_data_flag::EMERGING,
        pumpkin::plugin::player::BedrockStatusFlag::Sniffing => entity_data_flag::SNIFFING,
        pumpkin::plugin::player::BedrockStatusFlag::Digging => entity_data_flag::DIGGING,
        pumpkin::plugin::player::BedrockStatusFlag::SonicBoom => entity_data_flag::SONIC_BOOM,
        pumpkin::plugin::player::BedrockStatusFlag::HasDashTimeout => {
            entity_data_flag::HAS_DASH_TIMEOUT
        }
        pumpkin::plugin::player::BedrockStatusFlag::PushTowardsClosestSpace => {
            entity_data_flag::PUSH_TOWARDS_CLOSEST_SPACE
        }
        pumpkin::plugin::player::BedrockStatusFlag::Scenting => entity_data_flag::SCENTING,
        pumpkin::plugin::player::BedrockStatusFlag::Rising => entity_data_flag::RISING,
        pumpkin::plugin::player::BedrockStatusFlag::FeelingHappy => entity_data_flag::FEELING_HAPPY,
        pumpkin::plugin::player::BedrockStatusFlag::Searching => entity_data_flag::SEARCHING,
        pumpkin::plugin::player::BedrockStatusFlag::Crawling => entity_data_flag::CRAWLING,
        pumpkin::plugin::player::BedrockStatusFlag::BodyRotationBlocked => {
            entity_data_flag::BODY_ROTATION_BLOCKED
        }
        pumpkin::plugin::player::BedrockStatusFlag::RenderWhenInvisible => {
            entity_data_flag::RENDER_WHEN_INVISIBLE
        }
        pumpkin::plugin::player::BedrockStatusFlag::BodyRotationAxisAligned => {
            entity_data_flag::BODY_ROTATION_AXIS_ALIGNED
        }
        pumpkin::plugin::player::BedrockStatusFlag::Collidable => entity_data_flag::COLLIDABLE,
        pumpkin::plugin::player::BedrockStatusFlag::WasdAirControlled => {
            entity_data_flag::WASD_AIR_CONTROLLED
        }
        pumpkin::plugin::player::BedrockStatusFlag::DoesServerAuthOnlyDismount => {
            entity_data_flag::DOES_SERVER_AUTH_ONLY_DISMOUNT
        }
        pumpkin::plugin::player::BedrockStatusFlag::BodyRotationAlwaysFollowsHead => {
            entity_data_flag::BODY_ROTATION_ALWAYS_FOLLOWS_HEAD
        }
        pumpkin::plugin::player::BedrockStatusFlag::CanUseVerticalMovementAction => {
            entity_data_flag::CAN_USE_VERTICAL_MOVEMENT_ACTION
        }
        pumpkin::plugin::player::BedrockStatusFlag::RotationLockedToVehicle => {
            entity_data_flag::ROTATION_LOCKED_TO_VEHICLE
        }
    }
}

impl pumpkin::plugin::player::HostBedrockPlayer for PluginHostState {
    fn get_version(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
    ) -> wasmtime::Result<pumpkin::plugin::player::BedrockMinecraftVersion> {
        let player = self.get(&player)?;

        if let pumpkin_core::net::ClientPlatform::Bedrock(client) = player.client.as_ref() {
            Ok(to_wasm_bedrock_version(client.version.load()))
        } else {
            Ok(pumpkin::plugin::player::BedrockMinecraftVersion::Unknown)
        }
    }

    fn get_ability(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        ability: pumpkin::plugin::player::BedrockAbility,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?.clone();

        let ability = from_wasm_bedrock_ability(ability);
        let abilities = player
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match ability {
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Build
            | pumpkin_protocol::bedrock::client::update_abilities::Ability::Mine => {
                Ok(abilities.allow_modify_world)
            }
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Invulnerable => {
                Ok(abilities.invulnerable)
            }
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Flying => {
                Ok(abilities.flying)
            }
            pumpkin_protocol::bedrock::client::update_abilities::Ability::MayFly => {
                Ok(abilities.allow_flying)
            }
            pumpkin_protocol::bedrock::client::update_abilities::Ability::Instabuild => {
                Ok(abilities.creative)
            }
            _ => Ok(false), // Most Bedrock-specific abilities aren't tracked in generic Abilities struct yet
        }
    }

    fn set_ability(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        ability: pumpkin::plugin::player::BedrockAbility,
        value: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        let ability = from_wasm_bedrock_ability(ability);
        {
            let mut abilities = player
                .abilities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match ability {
                pumpkin_protocol::bedrock::client::update_abilities::Ability::Build
                | pumpkin_protocol::bedrock::client::update_abilities::Ability::Mine => {
                    abilities.allow_modify_world = value;
                }
                pumpkin_protocol::bedrock::client::update_abilities::Ability::Invulnerable => {
                    abilities.invulnerable = value;
                }
                pumpkin_protocol::bedrock::client::update_abilities::Ability::Flying => {
                    abilities.flying = value;
                }
                pumpkin_protocol::bedrock::client::update_abilities::Ability::MayFly => {
                    abilities.allow_flying = value;
                }
                pumpkin_protocol::bedrock::client::update_abilities::Ability::Instabuild => {
                    abilities.creative = value;
                }
                _ => {} // Not supported yet
            }
        };
        player.send_abilities_update();
        Ok(())
    }

    fn get_status_flag(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        flag: pumpkin::plugin::player::BedrockStatusFlag,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?.clone();

        let flag_index = from_wasm_bedrock_status_flag(flag);
        if flag_index < 64 {
            let flags = player
                .living_entity
                .entity
                .bedrock_flags
                .load(Ordering::Relaxed);
            Ok((flags & (1 << flag_index)) != 0)
        } else {
            let flags = player
                .living_entity
                .entity
                .bedrock_flags_two
                .load(Ordering::Relaxed);
            Ok((flags & (1 << (flag_index - 64))) != 0)
        }
    }

    async fn set_status_flag(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        flag: pumpkin::plugin::player::BedrockStatusFlag,
        value: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        let flag_index = from_wasm_bedrock_status_flag(flag);
        if flag_index < 64 {
            let mut flags = player
                .living_entity
                .entity
                .bedrock_flags
                .load(Ordering::Relaxed);
            if value {
                flags |= 1 << flag_index;
            } else {
                flags &= !(1 << flag_index);
            }
            player
                .living_entity
                .entity
                .bedrock_flags
                .store(flags, Ordering::Relaxed);
        } else {
            let mut flags = player
                .living_entity
                .entity
                .bedrock_flags_two
                .load(Ordering::Relaxed);
            if value {
                flags |= 1 << (flag_index - 64);
            } else {
                flags &= !(1 << (flag_index - 64));
            }
            player
                .living_entity
                .entity
                .bedrock_flags_two
                .store(flags, Ordering::Relaxed);
        }

        let mut metadata = SyncedActorDataList(std::collections::HashMap::new());
        metadata.set(
            entity_data_key::FLAGS,
            MetadataValue::Int64(
                player
                    .living_entity
                    .entity
                    .bedrock_flags
                    .load(Ordering::Relaxed),
            ),
        );
        metadata.set(
            entity_data_key::FLAGS_TWO,
            MetadataValue::Int64(
                player
                    .living_entity
                    .entity
                    .bedrock_flags_two
                    .load(Ordering::Relaxed),
            ),
        );

        let packet = CSetActorData {
            target_runtime_id: VarULong(player.get_entity().entity_id as u64),
            actor_data: metadata,
            synced_properties: PropertySyncData {
                int_entries_list: std::collections::HashMap::new(),
                float_entries_list: std::collections::HashMap::new(),
            },
            tick: VarULong(0),
        };

        if let pumpkin_core::net::ClientPlatform::Bedrock(client) = player.client.as_ref() {
            client.send_packet(&packet).await;
        }

        Ok(())
    }

    fn get_settings(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
    ) -> wasmtime::Result<pumpkin::plugin::player::BedrockPlayerSettings> {
        let player = self.get(&player)?.clone();

        if let pumpkin_core::net::ClientPlatform::Bedrock(client) = player.client.as_ref() {
            let data = client.client_data.load();
            (**data).as_ref().map_or_else(
                || Err(wasmtime::Error::msg("client data not available")),
                |data| {
                    Ok(pumpkin::plugin::player::BedrockPlayerSettings {
                        game_version: data.game_version.clone(),
                        device_os: to_wasm_bedrock_device_os(data.device_os),
                        device_id: data.device_id.clone(),
                        device_model: data.device_model.clone(),
                        language_code: data.language_code.clone(),
                        current_input_mode: to_wasm_bedrock_input_mode(data.current_input_mode),
                        default_input_mode: to_wasm_bedrock_input_mode(data.default_input_mode),
                        ui_profile: to_wasm_bedrock_ui_profile(data.ui_profile),
                        gui_scale: data.gui_scale,
                        is_editor_mode: data.client_is_editor_capable,
                        max_view_distance: data.max_view_distance,
                        memory_tier: data.memory_tier,
                        graphics_mode: to_wasm_bedrock_graphics_mode(data.graphics_mode),
                        playfab_id: data.play_fab_id.clone(),
                        client_random_id: data.client_random_id,
                        platform_offline_id: data.platform_offline_id.clone(),
                        platform_online_id: data.platform_online_id.clone(),
                        skin_id: data.skin_id.clone(),
                        arm_size: data.arm_size.clone(),
                        is_persona_skin: data.persona_skin,
                        is_premium_skin: data.premium_skin,
                        is_trusted_skin: data.trusted_skin,
                    })
                },
            )
        } else {
            Err(wasmtime::Error::msg("not a bedrock player"))
        }
    }

    async fn send_packet(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        packet: pumpkin::plugin::bedrock_packets::ClientboundPacket,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        if let Some(bytes) = crate::generated_packets::serialize_bedrock_packet(&packet) {
            player.client.send_packet_now_data(bytes).await;
        }
        Ok(())
    }

    async fn open_form(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        form: Form,
    ) -> wasmtime::Result<u32> {
        let player = self.get(&player)?.clone();

        if let pumpkin_core::net::ClientPlatform::Bedrock(client) = player.client.as_ref() {
            let form_id = client.next_form_id.fetch_add(1, Ordering::Relaxed);

            let locale_str = player.config.load().locale.clone();
            let locale = Locale::from_str(&locale_str).unwrap_or(Locale::EnUs);

            let form_json = match form {
                Form::Simple(simple) => self.serialize_simple_form(simple, locale),
                Form::Modal(modal) => self.serialize_modal_form(modal, locale),
                Form::Custom(custom) => self.serialize_custom_form(custom, locale),
            };

            client
                .send_packet(&CModalFormRequest {
                    form_id: pumpkin_protocol::codec::var_uint::VarUInt(form_id),
                    form_ui_json: form_json.to_string(),
                })
                .await;

            Ok(form_id)
        } else {
            Ok(0)
        }
    }

    fn get_scoreboard(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
    ) -> wasmtime::Result<Resource<pumpkin::plugin::scoreboard::BedrockScoreboard>> {
        let player = self.get(&player)?.clone();
        self.add(player)
    }

    fn reset_scoreboard(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();
        player.reset_scoreboard();
        Ok(())
    }

    async fn send_resource_packs_info(
        &mut self,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        info: pumpkin::plugin::player::BedrockResourcePacksInfo,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        if let pumpkin_core::net::ClientPlatform::Bedrock(client) = player.client.as_ref() {
            let entries = info
                .packs
                .into_iter()
                .map(
                    |p| pumpkin_protocol::bedrock::client::resource_packs_info::PackInfoData {
                        pack_id_version: PackIdVersion::new(Uuid::from_wit(&p.id), p.version),
                        pack_size: p.size,
                        cdn_url: p.download_url,
                        content_key: p.content_key.unwrap_or_default(),
                        subpack_name: p.sub_pack_name.unwrap_or_default(),
                        content_identity: p.content_id.unwrap_or_default(),
                        has_scripts: p.has_scripts,
                        is_addon_pack: p.addon_pack,
                        is_ray_tracing_capable: p.rtx_enabled,
                    },
                )
                .collect();

            let world_template_id = info
                .world_template_id
                .map_or_else(uuid::Uuid::nil, |id| Uuid::from_wit(&id));

            let packs_info =
                pumpkin_protocol::bedrock::client::resource_packs_info::CResourcePacksInfo {
                    resource_pack_required: info.required,
                    has_addon_packs: info.has_addon_packs,
                    has_scripts: info.has_scripts,
                    force_disable_vibrant_visuals: info.is_vibrant_visuals_force_disabled,
                    world_template_id_and_version: PackIdVersion::new(
                        world_template_id,
                        info.world_template_version.unwrap_or_default(),
                    ),
                    resource_packs: entries,
                };

            client.send_packet(&packs_info).await;
        }
        Ok(())
    }

    fn drop(
        &mut self,
        rep: Resource<pumpkin::plugin::player::BedrockPlayer>,
    ) -> wasmtime::Result<()> {
        self.drop(rep)
    }
}

impl pumpkin::plugin::player::HostBedrockPlayerWithStore<PluginHostState>
    for HasSelf<PluginHostState>
{
    async fn kick(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<pumpkin::plugin::player::BedrockPlayer>,
        options: pumpkin::plugin::player::BedrockKickOptions,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            let player = state.get(&player)?.clone();
            let plugin = state
                .plugin
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .ok_or_else(|| wasmtime::Error::msg("Plugin instance not available"))?;
            (player, plugin)
        };

        let disconnect_reason = from_wasm_bedrock_disconnect_reason(options.reason);
        if options.log_to_console {
            tracing::info!(
                "Kicking Bedrock player {} ({}) [Reason: {:?}]: {}",
                player.gameprofile.name,
                player.gameprofile.id,
                disconnect_reason,
                options.message
            );
        }

        let operation = async move {
            if let Some(server) = player.world().server.upgrade()
                && let Some(player_arc) = player.world().get_player_by_uuid(player.gameprofile.id)
            {
                let mut event =
                    pumpkin_core::plugin::api::events::player::player_kick::PlayerKickEvent::new(
                        player_arc,
                        options.message.clone(),
                    );
                server.plugin_manager.fire(&server, &mut event).await;
                if event.cancelled {
                    return Ok(());
                }
            }

            let send_packet = options.teardown_policy
                != pumpkin::plugin::player::SocketTeardownPolicy::DropConnection;
            if let pumpkin_core::net::ClientPlatform::Bedrock(bedrock) = player.client.as_ref() {
                bedrock
                    .kick_explicit(
                        disconnect_reason,
                        options.message,
                        options.skip_message,
                        options.filtered_message,
                        send_packet,
                    )
                    .await;
            }

            Ok(())
        };

        plugin.store.pump_reentry(&mut host, operation).await?
    }
}
