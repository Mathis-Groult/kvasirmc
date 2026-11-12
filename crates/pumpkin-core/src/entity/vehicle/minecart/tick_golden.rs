//! A record of what `MinecartEntity::tick` does, made before the function was split
//! (`tests/golden/minecart_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (64 blocks apart, rails
//! placed without neighbour updates so no rule rewrites their shape or power), its carts are
//! spawned into the world (the detector rail and the cart-to-cart push look them up there) and
//! ticked 100 times by calling `tick` directly, in spawn order. The scenes cover every rail
//! block and shape, a cart one block above each rail block, carts off rails (ground, falling,
//! water, lava, void), carts at rest, powered rails on and off, activator rails on and off under
//! each kind, the detector rail, TNT fuses and walls, player drivers, furnace, chest and hopper
//! carts, and two carts meeting.
//!
//! After every tick each cart is recorded: position, velocity, the last velocity sent to
//! watchers (`send_velocity` stores it, so every send shows), ground and wall flags, removed,
//! hurt time and direction, damage, its own NBT with sorted keys (fuel, push, fuse, items,
//! hopper switch) and its passengers' positions; plus the states of the scene's rail blocks and
//! the number of entities in the world, except in TNT scenes, where the explosion is random.
//!
//! `KVASIR_DUMP=<file>` appends every recorded tick as text, to find what changed.
//!
//! Not seen: packets other than velocity (wobble metadata, sounds, particles; nobody watches),
//! a plugin cancelling anything (the real server loads no plugin).

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::block_properties::{
    PoweredRailLikeProperties, RailLikeProperties, RailShape, RailShapeStraight,
};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_inventory::Inventory;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_protocol::java::server::play::SPlayerInput;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::MinecartEntity;
use crate::block::entities::chest::ChestBlockEntity;
use crate::entity::ai::test_support;
use crate::entity::item::ItemEntity;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;
/// A spawned cart and its riders.
type Spawned = (Arc<MinecartEntity>, Vec<Arc<dyn EntityBase>>);

const RAIL_Y: i32 = 64;
const TICKS: usize = 100;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(hash: u64, text: &str) -> u64 {
    let mut hash = hash;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[derive(Clone, Copy)]
enum Rails {
    Plain,
    Powered(bool),
    Activator(bool),
    Detector,
}

#[derive(Clone, Copy)]
enum Floor {
    Stone,
    Air,
    Water,
    Lava,
}

#[derive(Clone, Copy)]
enum Layout {
    /// `lanes` parallel tracks 3 blocks apart, 25 rails each; `wall` closes their far end.
    Line {
        rails: Rails,
        along_x: bool,
        lanes: i32,
        wall: bool,
    },
    /// A 7 x 5 rectangle of plain rails with the four corner shapes.
    Loop,
    /// A track that rises one block at its middle rail (12), toward + when `rising_positive`.
    Slope {
        rails: Rails,
        along_x: bool,
        rising_positive: bool,
    },
    /// One track along x: plain, powered, detector, plain, activator, unpowered powered.
    Mixed,
    /// No rails; a two-high wall across lanes 0-2 at `wall` when given.
    Bare { wall: Option<i32> },
}

#[derive(Clone, Copy)]
enum Rider {
    Zombie,
    Driver { input: i8, yaw: f32 },
}

#[derive(Clone, Copy)]
enum Setup {
    Plain,
    Fuel { fuel: i16, push_x: f64, push_z: f64 },
    Fuse(i32),
    Fill(usize),
    Loot,
    Disabled,
    HurtTime(i32),
    ChestAbove,
    ItemsAround,
}

#[derive(Clone, Copy)]
struct Cart {
    kind: &'static EntityType,
    /// Position along the layout's axis, lane, offset across it, height.
    along: f64,
    lane: i32,
    across: f64,
    y: f64,
    velocity: (f64, f64, f64),
    yaw: f32,
    setup: Setup,
    riders: &'static [Rider],
}

const fn cart(kind: &'static EntityType, along: f64, lane: i32, velocity: (f64, f64, f64)) -> Cart {
    Cart {
        kind,
        along,
        lane,
        across: 0.0,
        y: 64.0,
        velocity,
        yaw: 0.0,
        setup: Setup::Plain,
        riders: &[],
    }
}

const fn ride(along: f64, lane: i32, velocity: (f64, f64, f64)) -> Cart {
    cart(&EntityType::MINECART, along, lane, velocity)
}

const fn with_setup(cart: Cart, setup: Setup) -> Cart {
    Cart { setup, ..cart }
}

const fn with_riders(cart: Cart, riders: &'static [Rider]) -> Cart {
    Cart { riders, ..cart }
}

const fn at_height(cart: Cart, y: f64) -> Cart {
    Cart { y, ..cart }
}

const fn across(cart: Cart, across: f64) -> Cart {
    Cart { across, ..cart }
}

const fn facing(cart: Cart, yaw: f32) -> Cart {
    Cart { yaw, ..cart }
}

struct Scene {
    name: &'static str,
    floor: Floor,
    layout: Layout,
    carts: &'static [Cart],
    /// An explosion may happen: its block and entity effects are random, so leave them out.
    tnt: bool,
}

const fn line(rails: Rails, along_x: bool, lanes: i32) -> Layout {
    Layout::Line {
        rails,
        along_x,
        lanes,
        wall: false,
    }
}

const fn scene(name: &'static str, layout: Layout, carts: &'static [Cart]) -> Scene {
    Scene {
        name,
        floor: Floor::Stone,
        layout,
        carts,
        tnt: false,
    }
}

const fn off_rails(name: &'static str, floor: Floor, carts: &'static [Cart]) -> Scene {
    Scene {
        name,
        floor,
        layout: Layout::Bare { wall: None },
        carts,
        tnt: false,
    }
}

const fn tnt(name: &'static str, layout: Layout, carts: &'static [Cart]) -> Scene {
    Scene {
        name,
        floor: Floor::Stone,
        layout,
        carts,
        tnt: true,
    }
}

const PLAIN_X: Layout = line(Rails::Plain, true, 1);
const FORWARD: i8 = SPlayerInput::FORWARD;
const BACKWARD: i8 = SPlayerInput::BACKWARD;
const TNT_CART: &EntityType = &EntityType::TNT_MINECART;
const CHEST_CART: &EntityType = &EntityType::CHEST_MINECART;
const HOPPER_CART: &EntityType = &EntityType::HOPPER_MINECART;
const FURNACE_CART: &EntityType = &EntityType::FURNACE_MINECART;
const FUELLED: Setup = Setup::Fuel {
    fuel: 100,
    push_x: 1.0,
    push_z: 0.0,
};

const SCENES: &[Scene] = &[
    // Plain rails, every shape.
    scene("rail_x_east", PLAIN_X, &[ride(2.5, 0, (0.3, 0.0, 0.0))]),
    scene("rail_x_west", PLAIN_X, &[ride(22.5, 0, (-0.3, 0.0, 0.0))]),
    scene(
        "rail_z_south",
        line(Rails::Plain, false, 1),
        &[ride(2.5, 0, (0.0, 0.0, 0.3))],
    ),
    scene(
        "rail_z_north",
        line(Rails::Plain, false, 1),
        &[ride(22.5, 0, (0.0, 0.0, -0.3))],
    ),
    scene(
        "rail_x_off_centre",
        PLAIN_X,
        &[at_height(across(ride(2.5, 0, (0.2, 0.0, 0.15)), 0.3), 64.4)],
    ),
    scene(
        "loop_top_east",
        Layout::Loop,
        &[ride(3.5, 0, (0.4, 0.0, 0.0))],
    ),
    scene(
        "loop_top_west",
        Layout::Loop,
        &[ride(3.5, 0, (-0.4, 0.0, 0.0))],
    ),
    scene(
        "loop_bottom_west",
        Layout::Loop,
        &[across(ride(3.5, 0, (-0.4, 0.0, 0.0)), 4.0)],
    ),
    scene(
        "loop_bottom_east",
        Layout::Loop,
        &[across(ride(3.5, 0, (0.4, 0.0, 0.0)), 4.0)],
    ),
    scene(
        "slope_up_east",
        Layout::Slope {
            rails: Rails::Plain,
            along_x: true,
            rising_positive: true,
        },
        &[ride(9.5, 0, (0.4, 0.0, 0.0))],
    ),
    scene(
        "slope_down_east",
        Layout::Slope {
            rails: Rails::Plain,
            along_x: true,
            rising_positive: true,
        },
        &[at_height(ride(14.5, 0, (-0.3, 0.0, 0.0)), 65.0)],
    ),
    scene(
        "slope_up_west",
        Layout::Slope {
            rails: Rails::Plain,
            along_x: true,
            rising_positive: false,
        },
        &[ride(14.5, 0, (-0.4, 0.0, 0.0))],
    ),
    scene(
        "slope_down_west",
        Layout::Slope {
            rails: Rails::Plain,
            along_x: true,
            rising_positive: false,
        },
        &[at_height(ride(9.5, 0, (0.3, 0.0, 0.0)), 65.0)],
    ),
    scene(
        "slope_up_south",
        Layout::Slope {
            rails: Rails::Plain,
            along_x: false,
            rising_positive: true,
        },
        &[ride(9.5, 0, (0.0, 0.0, 0.4))],
    ),
    scene(
        "slope_up_north",
        Layout::Slope {
            rails: Rails::Plain,
            along_x: false,
            rising_positive: false,
        },
        &[ride(14.5, 0, (0.0, 0.0, -0.4))],
    ),
    // A cart whose own block is air, one block above each rail block.
    scene(
        "above_plain",
        PLAIN_X,
        &[at_height(ride(4.5, 0, (0.2, 0.0, 0.0)), 65.2)],
    ),
    scene(
        "above_powered",
        line(Rails::Powered(true), true, 1),
        &[at_height(ride(4.5, 0, (0.2, 0.0, 0.0)), 65.2)],
    ),
    scene(
        "above_detector",
        line(Rails::Detector, true, 1),
        &[at_height(ride(4.5, 0, (0.2, 0.0, 0.0)), 65.2)],
    ),
    scene(
        "above_activator",
        line(Rails::Activator(true), true, 1),
        &[at_height(ride(4.5, 0, (0.2, 0.0, 0.0)), 65.2)],
    ),
    // Carts WITH passengers on powered activator rails are the last scenes: the tick deadlocked
    // on the passenger lock it held until the fix of card #31.
    // Off rails.
    off_rails("ground", Floor::Stone, &[ride(4.5, 0, (0.3, 0.0, 0.1))]),
    off_rails(
        "falling_to_ground",
        Floor::Stone,
        &[at_height(ride(4.5, 0, (0.1, 0.0, 0.0)), 70.0)],
    ),
    off_rails("over_water", Floor::Water, &[ride(4.5, 0, (0.2, 0.0, 0.0))]),
    off_rails("over_lava", Floor::Lava, &[ride(4.5, 0, (0.2, 0.0, 0.0))]),
    off_rails(
        "void",
        Floor::Air,
        &[at_height(ride(4.5, 0, (0.1, 0.0, 0.0)), 70.0)],
    ),
    off_rails(
        "chest_on_ground",
        Floor::Stone,
        &[with_setup(
            cart(CHEST_CART, 4.5, 0, (0.3, 0.0, 0.0)),
            Setup::Fill(13),
        )],
    ),
    off_rails(
        "furnace_on_ground",
        Floor::Stone,
        &[with_setup(
            cart(FURNACE_CART, 4.5, 0, (0.2, 0.0, 0.0)),
            FUELLED,
        )],
    ),
    // At rest and nearly at rest.
    scene(
        "rest",
        line(Rails::Plain, true, 3),
        &[
            ride(4.5, 0, (0.0, 0.0, 0.0)),
            ride(4.5, 1, (0.0005, 0.0, 0.0)),
            ride(4.5, 2, (0.004, 0.0, 0.0)),
        ],
    ),
    // Powered rails.
    scene(
        "powered_on_moving",
        line(Rails::Powered(true), true, 2),
        &[
            ride(2.5, 0, (0.2, 0.0, 0.0)),
            ride(20.5, 1, (-0.05, 0.0, 0.0)),
        ],
    ),
    scene(
        "powered_on_rest_x",
        line(Rails::Powered(true), true, 5),
        &[
            facing(ride(12.5, 0, (0.0, 0.0, 0.0)), 0.0),
            facing(ride(12.5, 1, (0.0, 0.0, 0.0)), 90.0),
            facing(ride(12.5, 2, (0.0, 0.0, 0.0)), 180.0),
            facing(ride(12.5, 3, (0.0, 0.0, 0.0)), 270.0),
            facing(ride(12.5, 4, (0.0, 0.0, 0.0)), 45.0),
        ],
    ),
    scene(
        "powered_on_rest_z",
        line(Rails::Powered(true), false, 3),
        &[
            facing(ride(12.5, 0, (0.0, 0.0, 0.0)), 0.0),
            facing(ride(12.5, 1, (0.0, 0.0, 0.0)), 180.0),
            facing(ride(12.5, 2, (0.0, 0.0, 0.0)), 90.0),
        ],
    ),
    scene(
        "powered_off",
        line(Rails::Powered(false), true, 3),
        &[
            ride(2.5, 0, (0.3, 0.0, 0.0)),
            ride(12.5, 1, (0.015, 0.0, 0.0)),
            ride(20.5, 2, (-0.2, 0.0, 0.0)),
        ],
    ),
    scene(
        "powered_slope_east",
        Layout::Slope {
            rails: Rails::Powered(true),
            along_x: true,
            rising_positive: true,
        },
        &[ride(9.5, 0, (0.1, 0.0, 0.0))],
    ),
    scene(
        "powered_slope_west",
        Layout::Slope {
            rails: Rails::Powered(true),
            along_x: true,
            rising_positive: false,
        },
        &[ride(14.5, 0, (-0.1, 0.0, 0.0))],
    ),
    scene(
        "powered_slope_south",
        Layout::Slope {
            rails: Rails::Powered(true),
            along_x: false,
            rising_positive: true,
        },
        &[ride(9.5, 0, (0.0, 0.0, 0.1))],
    ),
    scene(
        "powered_slope_north",
        Layout::Slope {
            rails: Rails::Powered(true),
            along_x: false,
            rising_positive: false,
        },
        &[ride(14.5, 0, (0.0, 0.0, -0.1))],
    ),
    // Activator rails.
    scene(
        "activator_on",
        line(Rails::Activator(true), true, 5),
        &[
            ride(4.5, 0, (0.0, 0.0, 0.0)),
            with_setup(ride(4.5, 1, (0.0, 0.0, 0.0)), Setup::HurtTime(5)),
            ride(4.5, 2, (0.1, 0.0, 0.0)),
            cart(HOPPER_CART, 4.5, 3, (0.0, 0.0, 0.0)),
            cart(CHEST_CART, 4.5, 4, (0.1, 0.0, 0.0)),
        ],
    ),
    scene(
        "activator_off",
        line(Rails::Activator(false), true, 3),
        &[
            with_riders(ride(4.5, 0, (0.0, 0.0, 0.0)), &[Rider::Zombie]),
            with_setup(cart(HOPPER_CART, 4.5, 1, (0.0, 0.0, 0.0)), Setup::Disabled),
            cart(TNT_CART, 4.5, 2, (0.0, 0.0, 0.0)),
        ],
    ),
    scene(
        "activator_slope_east",
        Layout::Slope {
            rails: Rails::Activator(true),
            along_x: true,
            rising_positive: true,
        },
        &[ride(9.5, 0, (0.3, 0.0, 0.0))],
    ),
    tnt(
        "activator_on_tnt",
        line(Rails::Activator(true), true, 1),
        &[cart(TNT_CART, 4.5, 0, (0.0, 0.0, 0.0))],
    ),
    // Detector rails.
    scene(
        "detector",
        line(Rails::Detector, true, 2),
        &[
            ride(2.5, 0, (0.2, 0.0, 0.0)),
            ride(10.5, 1, (0.0, 0.0, 0.0)),
        ],
    ),
    scene(
        "detector_slope_north",
        Layout::Slope {
            rails: Rails::Detector,
            along_x: false,
            rising_positive: false,
        },
        &[ride(14.5, 0, (0.0, 0.0, -0.3))],
    ),
    scene("mixed", Layout::Mixed, &[ride(0.5, 0, (0.4, 0.0, 0.0))]),
    // TNT.
    tnt(
        "tnt_fuse",
        PLAIN_X,
        &[with_setup(
            cart(TNT_CART, 4.5, 0, (0.0, 0.0, 0.0)),
            Setup::Fuse(5),
        )],
    ),
    tnt(
        "tnt_wall_fast",
        Layout::Line {
            rails: Rails::Plain,
            along_x: true,
            lanes: 1,
            wall: true,
        },
        &[cart(TNT_CART, 21.5, 0, (0.4, 0.0, 0.0))],
    ),
    tnt(
        "tnt_wall_slow",
        Layout::Line {
            rails: Rails::Plain,
            along_x: true,
            lanes: 1,
            wall: true,
        },
        &[cart(TNT_CART, 23.5, 0, (0.08, 0.0, 0.0))],
    ),
    tnt(
        "tnt_ground_wall",
        Layout::Bare { wall: Some(5) },
        &[cart(TNT_CART, 4.5, 0, (0.4, 0.0, 0.0))],
    ),
    // Drivers.
    scene(
        "driver_forward",
        PLAIN_X,
        &[with_riders(
            ride(4.5, 0, (0.0, 0.0, 0.0)),
            &[Rider::Driver {
                input: FORWARD,
                yaw: -90.0,
            }],
        )],
    ),
    scene(
        "driver_backward",
        PLAIN_X,
        &[with_riders(
            ride(20.5, 0, (0.0, 0.0, 0.0)),
            &[Rider::Driver {
                input: BACKWARD,
                yaw: -90.0,
            }],
        )],
    ),
    scene(
        "driver_both",
        PLAIN_X,
        &[with_riders(
            ride(4.5, 0, (0.0, 0.0, 0.0)),
            &[Rider::Driver {
                input: FORWARD | BACKWARD,
                yaw: -90.0,
            }],
        )],
    ),
    scene(
        "driver_idle",
        PLAIN_X,
        &[with_riders(
            ride(4.5, 0, (0.2, 0.0, 0.0)),
            &[Rider::Driver {
                input: 0,
                yaw: -90.0,
            }],
        )],
    ),
    scene(
        "driver_speeds",
        line(Rails::Plain, true, 3),
        &[
            with_riders(
                ride(2.5, 0, (0.35, 0.0, 0.0)),
                &[Rider::Driver {
                    input: FORWARD,
                    yaw: -90.0,
                }],
            ),
            with_riders(
                ride(2.5, 1, (0.5, 0.0, 0.0)),
                &[Rider::Driver {
                    input: FORWARD,
                    yaw: -90.0,
                }],
            ),
            with_riders(
                ride(2.5, 2, (0.1, 0.0, 0.0)),
                &[Rider::Driver {
                    input: FORWARD,
                    yaw: -90.0,
                }],
            ),
        ],
    ),
    scene(
        "driver_diagonal",
        line(Rails::Plain, false, 1),
        &[with_riders(
            ride(4.5, 0, (0.0, 0.0, 0.0)),
            &[Rider::Driver {
                input: FORWARD,
                yaw: -30.0,
            }],
        )],
    ),
    off_rails(
        "driver_on_ground",
        Floor::Stone,
        &[with_riders(
            ride(4.5, 0, (0.0, 0.0, 0.0)),
            &[Rider::Driver {
                input: FORWARD,
                yaw: -90.0,
            }],
        )],
    ),
    scene(
        "driver_behind_zombie",
        PLAIN_X,
        &[with_riders(
            ride(4.5, 0, (0.0, 0.0, 0.0)),
            &[
                Rider::Zombie,
                Rider::Driver {
                    input: FORWARD,
                    yaw: -90.0,
                },
            ],
        )],
    ),
    scene(
        "passenger_zombie",
        PLAIN_X,
        &[with_riders(ride(2.5, 0, (0.3, 0.0, 0.0)), &[Rider::Zombie])],
    ),
    // Furnace, chest and hopper carts.
    scene(
        "furnace",
        line(Rails::Plain, true, 4),
        &[
            with_setup(cart(FURNACE_CART, 2.5, 0, (0.1, 0.0, 0.0)), FUELLED),
            with_setup(
                cart(FURNACE_CART, 2.5, 1, (0.2, 0.0, 0.0)),
                Setup::Fuel {
                    fuel: 0,
                    push_x: 0.0,
                    push_z: 0.0,
                },
            ),
            with_setup(
                cart(FURNACE_CART, 2.5, 2, (0.2, 0.0, 0.0)),
                Setup::Fuel {
                    fuel: 100,
                    push_x: -1.0,
                    push_z: 0.0,
                },
            ),
            with_setup(
                cart(FURNACE_CART, 2.5, 3, (0.1, 0.0, 0.0)),
                Setup::Fuel {
                    fuel: 3,
                    push_x: 0.005,
                    push_z: 0.0,
                },
            ),
        ],
    ),
    scene(
        "chests",
        line(Rails::Plain, true, 4),
        &[
            cart(CHEST_CART, 2.5, 0, (0.3, 0.0, 0.0)),
            with_setup(cart(CHEST_CART, 2.5, 1, (0.3, 0.0, 0.0)), Setup::Fill(13)),
            with_setup(cart(CHEST_CART, 2.5, 2, (0.3, 0.0, 0.0)), Setup::Fill(27)),
            with_setup(cart(CHEST_CART, 2.5, 3, (0.3, 0.0, 0.0)), Setup::Loot),
        ],
    ),
    scene(
        "hopper_under_chest",
        PLAIN_X,
        &[with_setup(
            cart(HOPPER_CART, 4.5, 0, (0.0, 0.0, 0.0)),
            Setup::ChestAbove,
        )],
    ),
    scene(
        "hopper_items_around",
        PLAIN_X,
        &[with_setup(
            cart(HOPPER_CART, 4.5, 0, (0.0, 0.0, 0.0)),
            Setup::ItemsAround,
        )],
    ),
    scene(
        "hopper_moving_full",
        PLAIN_X,
        &[with_setup(
            cart(HOPPER_CART, 2.5, 0, (0.3, 0.0, 0.0)),
            Setup::Fill(5),
        )],
    ),
    // Carts meeting.
    scene(
        "collision",
        PLAIN_X,
        &[
            ride(2.5, 0, (0.4, 0.0, 0.0)),
            cart(CHEST_CART, 8.5, 0, (0.0, 0.0, 0.0)),
        ],
    ),
    scene(
        "collision_furnace",
        PLAIN_X,
        &[
            with_setup(cart(FURNACE_CART, 2.5, 0, (0.3, 0.0, 0.0)), FUELLED),
            ride(8.5, 0, (0.0, 0.0, 0.0)),
        ],
    ),
    // Passengers on powered activator rails (added with the fix of card #31).
    scene(
        "activator_on_riders",
        line(Rails::Activator(true), true, 3),
        &[
            with_riders(ride(4.5, 0, (0.0, 0.0, 0.0)), &[Rider::Zombie]),
            with_riders(
                ride(2.5, 1, (0.1, 0.0, 0.0)),
                &[
                    Rider::Driver {
                        input: 0,
                        yaw: -90.0,
                    },
                    Rider::Zombie,
                ],
            ),
            with_setup(
                with_riders(ride(4.5, 2, (0.0, 0.0, 0.0)), &[Rider::Zombie]),
                Setup::HurtTime(5),
            ),
        ],
    ),
    scene(
        "above_activator_rider",
        line(Rails::Activator(true), true, 1),
        &[at_height(
            with_riders(ride(4.5, 0, (0.2, 0.0, 0.0)), &[Rider::Zombie]),
            65.2,
        )],
    ),
];

/// One scene's area: its corner and the rails placed in it.
struct Area<'a> {
    world: &'a Arc<World>,
    x: i32,
    z: i32,
    rails: Vec<BlockPos>,
}

impl Area<'_> {
    fn put(&self, x: i32, y: i32, z: i32, state: BlockStateId) {
        self.world.set_block_state(
            &BlockPos(Vector3::new(self.x + x, y, self.z + z)),
            state,
            BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
    }

    fn rail(&mut self, x: i32, y: i32, z: i32, state: BlockStateId) {
        self.put(x, y, z, state);
        self.rails
            .push(BlockPos(Vector3::new(self.x + x, y, self.z + z)));
    }

    /// The block at `along` on `lane` of a track along x or z.
    fn track(&mut self, along_x: bool, lane: i32, along: i32, y: i32, state: BlockStateId) {
        let (x, z) = if along_x {
            (along, lane * 3)
        } else {
            (lane * 3, along)
        };
        self.rail(x, y, z, state);
    }

    fn support(&self, along_x: bool, along: i32, y: i32) {
        let (x, z) = if along_x { (along, 0) } else { (0, along) };
        self.put(x, y, z, Block::STONE.default_state.id);
    }
}

fn plain(shape: RailShape) -> BlockStateId {
    let mut props = RailLikeProperties::default(&Block::RAIL);
    props.shape = shape;
    props.to_state_id(&Block::RAIL)
}

fn straight(block: &Block, shape: RailShapeStraight, powered: bool) -> BlockStateId {
    let mut props = PoweredRailLikeProperties::default(block);
    props.shape = shape;
    props.powered = powered;
    props.to_state_id(block)
}

/// The state of `rails` with the plain shape `flat` (straight shapes are the same names).
fn rail_state(rails: Rails, flat: RailShape, straight_shape: RailShapeStraight) -> BlockStateId {
    match rails {
        Rails::Plain => plain(flat),
        Rails::Powered(on) => straight(&Block::POWERED_RAIL, straight_shape, on),
        Rails::Activator(on) => straight(&Block::ACTIVATOR_RAIL, straight_shape, on),
        Rails::Detector => straight(&Block::DETECTOR_RAIL, straight_shape, false),
    }
}

fn flat_state(rails: Rails, along_x: bool) -> BlockStateId {
    if along_x {
        rail_state(rails, RailShape::EastWest, RailShapeStraight::EastWest)
    } else {
        rail_state(rails, RailShape::NorthSouth, RailShapeStraight::NorthSouth)
    }
}

fn build(area: &mut Area<'_>, floor: Floor, layout: Layout) {
    let floor_state = match floor {
        Floor::Stone => Some(Block::STONE.default_state.id),
        Floor::Air => None,
        Floor::Water => Some(Block::WATER.default_state.id),
        Floor::Lava => Some(Block::LAVA.default_state.id),
    };
    if let Some(state) = floor_state {
        // Under the tracks and the carts' paths only: every placed block costs about a
        // millisecond in a debug build.
        let (along_x, lanes) = match layout {
            Layout::Line { along_x, lanes, .. } => (along_x, lanes),
            Layout::Slope { along_x, .. } => (along_x, 1),
            Layout::Loop => (true, 2),
            Layout::Mixed => (true, 1),
            Layout::Bare { .. } => (true, 3),
        };
        for along in -2..=27 {
            for across in -1..lanes * 3 {
                let (x, z) = if along_x {
                    (along, across)
                } else {
                    (across, along)
                };
                area.put(x, RAIL_Y - 1, z, state);
            }
        }
    }
    match layout {
        Layout::Line {
            rails,
            along_x,
            lanes,
            wall,
        } => {
            for lane in 0..lanes {
                for along in 0..=24 {
                    area.track(along_x, lane, along, RAIL_Y, flat_state(rails, along_x));
                }
                if wall {
                    for y in [RAIL_Y, RAIL_Y + 1] {
                        let (x, z) = if along_x {
                            (25, lane * 3)
                        } else {
                            (lane * 3, 25)
                        };
                        area.put(x, y, z, Block::STONE.default_state.id);
                    }
                }
            }
        }
        Layout::Loop => build_loop(area),
        Layout::Slope {
            rails,
            along_x,
            rising_positive,
        } => build_slope(area, rails, along_x, rising_positive),
        Layout::Mixed => {
            for along in 0..=24 {
                let rails = match along {
                    6..=10 => Rails::Powered(true),
                    11 => Rails::Detector,
                    16..=18 => Rails::Activator(true),
                    19..=24 => Rails::Powered(false),
                    _ => Rails::Plain,
                };
                area.track(true, 0, along, RAIL_Y, flat_state(rails, true));
            }
        }
        Layout::Bare { wall } => {
            if let Some(at) = wall {
                for lane in 0..=2 {
                    for y in [RAIL_Y, RAIL_Y + 1] {
                        area.put(at, y, lane * 3, Block::STONE.default_state.id);
                    }
                }
            }
        }
    }
}

fn build_loop(area: &mut Area<'_>) {
    let east_west = plain(RailShape::EastWest);
    let north_south = plain(RailShape::NorthSouth);
    for x in 1..=5 {
        area.rail(x, RAIL_Y, 0, east_west);
        area.rail(x, RAIL_Y, 4, east_west);
    }
    for z in 1..=3 {
        area.rail(0, RAIL_Y, z, north_south);
        area.rail(6, RAIL_Y, z, north_south);
    }
    area.rail(0, RAIL_Y, 0, plain(RailShape::SouthEast));
    area.rail(6, RAIL_Y, 0, plain(RailShape::SouthWest));
    area.rail(6, RAIL_Y, 4, plain(RailShape::NorthWest));
    area.rail(0, RAIL_Y, 4, plain(RailShape::NorthEast));
}

fn build_slope(area: &mut Area<'_>, rails: Rails, along_x: bool, rising_positive: bool) {
    let flat = flat_state(rails, along_x);
    let ascending = match (along_x, rising_positive) {
        (true, true) => rail_state(
            rails,
            RailShape::AscendingEast,
            RailShapeStraight::AscendingEast,
        ),
        (true, false) => rail_state(
            rails,
            RailShape::AscendingWest,
            RailShapeStraight::AscendingWest,
        ),
        (false, true) => rail_state(
            rails,
            RailShape::AscendingSouth,
            RailShapeStraight::AscendingSouth,
        ),
        (false, false) => rail_state(
            rails,
            RailShape::AscendingNorth,
            RailShapeStraight::AscendingNorth,
        ),
    };
    for along in 0..=24 {
        let high = if rising_positive {
            along > 12
        } else {
            along < 12
        };
        if along == 12 {
            area.track(along_x, 0, along, RAIL_Y, ascending);
        } else if high {
            area.support(along_x, along, RAIL_Y);
            area.track(along_x, 0, along, RAIL_Y + 1, flat);
        } else {
            area.track(along_x, 0, along, RAIL_Y, flat);
        }
    }
}

/// Places `spec` in the area, sets it up, spawns it and seats its riders.
fn spawn(
    area: &Area<'_>,
    layout: Layout,
    spec: &Cart,
) -> Result<Spawned, Box<dyn std::error::Error>> {
    let world = area.world;
    let along_x = match layout {
        Layout::Line { along_x, .. } | Layout::Slope { along_x, .. } => along_x,
        Layout::Loop | Layout::Mixed | Layout::Bare { .. } => true,
    };
    let across = f64::from(spec.lane * 3) + 0.5 + spec.across;
    let (x, z) = if along_x {
        (spec.along, across)
    } else {
        (across, spec.along)
    };
    let at = Vector3::new(f64::from(area.x) + x, spec.y, f64::from(area.z) + z);
    let cart = Arc::new(MinecartEntity::new(Entity::new(
        Arc::clone(world),
        at,
        spec.kind,
    )));
    let entity = cart.get_entity();
    entity.velocity.store(Vector3::new(
        spec.velocity.0,
        spec.velocity.1,
        spec.velocity.2,
    ));
    entity.yaw.store(spec.yaw);
    set_up(world, &cart, spec.setup, at);
    let as_dyn: Arc<dyn EntityBase> = Arc::clone(&cart) as Arc<dyn EntityBase>;
    if !world.spawn_entity(Arc::clone(&as_dyn)) {
        return Err(format!("{} was not spawned", spec.kind.resource_name).into());
    }
    let mut riders: Vec<Arc<dyn EntityBase>> = Vec::new();
    for rider in spec.riders {
        let seat = Vector3::new(at.x, at.y + 0.6, at.z);
        let rider: Arc<dyn EntityBase> = match rider {
            Rider::Zombie => Arc::new(LivingEntity::new(Entity::new(
                Arc::clone(world),
                seat,
                &EntityType::ZOMBIE,
            ))),
            Rider::Driver { input, yaw } => {
                let player = test_support::java_player(world, GameMode::Survival);
                player.get_entity().set_pos(seat);
                player.get_entity().yaw.store(*yaw);
                player.last_input.store(*input, Relaxed);
                player
            }
        };
        entity.add_passenger_silently(Arc::clone(&as_dyn), Arc::clone(&rider));
        riders.push(rider);
    }
    Ok((cart, riders))
}

fn set_up(world: &Arc<World>, cart: &MinecartEntity, setup: Setup, at: Vector3<f64>) {
    let mut nbt = NbtCompound::new();
    match setup {
        Setup::Plain => return,
        Setup::Fuel {
            fuel,
            push_x,
            push_z,
        } => {
            nbt.put_short("Fuel", fuel);
            nbt.put_double("PushX", push_x);
            nbt.put_double("PushZ", push_z);
        }
        Setup::Fuse(fuse) => nbt.put_int("fuse", fuse),
        Setup::Loot => nbt.put_string("LootTable", "minecraft:chests/abandoned_mineshaft".into()),
        Setup::Disabled => nbt.put_bool("Enabled", false),
        Setup::HurtTime(time) => {
            cart.vehicle.set_hurt_time(time);
            cart.vehicle.set_damage(12.0);
            return;
        }
        Setup::Fill(stacks) => {
            if let Some(inventory) = cart.container() {
                for slot in 0..stacks.min(inventory.size()) {
                    inventory.set_stack(slot, ItemStack::new(64, &Item::COBBLESTONE));
                }
            }
            return;
        }
        Setup::ChestAbove => {
            let position = BlockPos::floored(at.x, at.y + 1.5, at.z);
            world.set_block_state(
                &position,
                Block::CHEST.default_state.id,
                BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
            );
            let chest = Arc::new(ChestBlockEntity::new(position));
            chest.set_stack(0, ItemStack::new(3, &Item::OAK_LOG));
            chest.set_stack(5, ItemStack::new(2, &Item::IRON_INGOT));
            world.add_block_entity(chest);
            return;
        }
        Setup::ItemsAround => {
            for (dx, dy, count) in [(0.0, 1.0, 2), (0.6, 0.1, 5), (2.0, 0.1, 7)] {
                let item = ItemEntity::new(
                    Entity::new(
                        Arc::clone(world),
                        Vector3::new(at.x + dx, at.y + dy, at.z),
                        &EntityType::ITEM,
                    ),
                    ItemStack::new(count, &Item::DIAMOND),
                );
                world.spawn_entity(Arc::new(item));
            }
            return;
        }
    }
    cart.read_custom_nbt(&nbt);
}

/// NBT as text with the compound keys sorted (a compound is a `HashMap`).
fn nbt_text(tag: &NbtTag, text: &mut String) {
    match tag {
        NbtTag::Compound(compound) => compound_text(compound, text),
        NbtTag::List(items) => {
            text.push('[');
            for item in items {
                nbt_text(item, text);
                text.push(',');
            }
            text.push(']');
        }
        other => {
            let _ = write!(text, "{other:?}");
        }
    }
}

fn compound_text(compound: &NbtCompound, text: &mut String) {
    let mut keys: Vec<&str> = compound.child_tags.keys().map(AsRef::as_ref).collect();
    keys.sort_unstable();
    text.push('{');
    for key in keys {
        let _ = write!(text, "{key}:");
        if let Some(tag) = compound.child_tags.get(key) {
            nbt_text(tag, text);
        }
        text.push(',');
    }
    text.push('}');
}

fn cart_text(cart: &MinecartEntity, riders: &[Arc<dyn EntityBase>]) -> String {
    let entity = cart.get_entity();
    let mut text = format!(
        "{:?} {:?} sent {:?} ground {} wall {} removed {} hurt {} {} damage {:?} ",
        entity.pos.load(),
        entity.velocity.load(),
        entity.last_sent_velocity.load(),
        entity.on_ground.load(Relaxed),
        entity.horizontal_collision.load(Relaxed),
        entity.is_removed(),
        cart.vehicle.get_hurt_time(),
        cart.vehicle.get_hurt_dir(),
        cart.vehicle.get_damage(),
    );
    let mut nbt = NbtCompound::new();
    cart.write_custom_nbt(&mut nbt);
    compound_text(&nbt, &mut text);
    for rider in riders {
        let _ = write!(
            text,
            " [{:?} rides {}]",
            rider.get_entity().pos.load(),
            entity.has_passenger(rider.get_entity().entity_id),
        );
    }
    text
}

/// A short readable end state of a cart for the scene's summary line.
fn summary(cart: &MinecartEntity, riders: &[Arc<dyn EntityBase>]) -> String {
    let entity = cart.get_entity();
    let pos = entity.pos.load();
    let velocity = entity.velocity.load();
    let riding = riders
        .iter()
        .filter(|r| entity.has_passenger(r.get_entity().entity_id))
        .count();
    format!(
        "({:.2} {:.2} {:.2}) v {:.3}{}{}",
        pos.x,
        pos.y,
        pos.z,
        velocity.length(),
        if entity.is_removed() { " removed" } else { "" },
        if riders.is_empty() {
            String::new()
        } else {
            format!(" riders {riding}/{}", riders.len())
        },
    )
}

fn run_scene(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    report: &mut String,
) -> TestResult {
    let chunk_x = index as i32 * 4;
    for cx in chunk_x..chunk_x + 4 {
        for cz in 0..4 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mut area = Area {
        world,
        x: chunk_x * 16 + 16,
        z: 16,
        rails: Vec::new(),
    };
    build(&mut area, scene.floor, scene.layout);
    let mut carts = Vec::new();
    for spec in scene.carts {
        carts.push(spawn(&area, scene.layout, spec)?);
    }

    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for tick in 1..=TICKS {
        for (cart, _) in &carts {
            if !cart.get_entity().is_removed() {
                EntityBase::tick(cart.as_ref(), cart.as_ref(), server);
            }
        }
        let mut text = String::new();
        for (cart, riders) in &carts {
            text.push_str(&cart_text(cart, riders));
            text.push('|');
        }
        if !scene.tnt {
            for rail in &area.rails {
                let _ = write!(text, "{},", world.get_block_state_id(rail));
            }
            let _ = write!(text, " entities {}", world.entities.load().len());
        }
        if let Some(dump) = std::env::var_os("KVASIR_DUMP") {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dump)?;
            writeln!(file, "{} {tick} {text}", scene.name)?;
        }
        hash = fnv(hash, &text);
        if tick % 25 == 0 {
            let _ = writeln!(
                report,
                "{} ticks {}-{tick}: {hash:016x}",
                scene.name,
                tick - 24
            );
        }
    }
    let ends: Vec<String> = carts
        .iter()
        .map(|(cart, riders)| summary(cart, riders))
        .collect();
    let _ = writeln!(report, "{} end: {}", scene.name, ends.join(", "));

    for entity in world.entities.load().iter() {
        world.remove_entity(entity.as_ref());
    }
    for cx in chunk_x..chunk_x + 4 {
        for cz in 0..4 {
            world.level.loaded_chunks.remove(&Vector2::new(cx, cz));
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn minecart_tick() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let mut report = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run_scene(&server, &world, index, scene, &mut report)?;
    }
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("minecart_tick.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
