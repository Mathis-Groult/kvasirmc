//! The first pass of the sky light: full light above every column, light that fades through the
//! blocks of each column, and the cells beside a taller neighbor that the light must still enter.

use pumpkin_data::{BlockDirection, BlockStateId};
use pumpkin_util::HeightMap;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;

use super::engine::{PropagationEntry, SkyLightPropagator};
use super::storage::get_sky_light;
use crate::chunk_system::Chunk;
use crate::chunk_system::generation_cache::Cache;
use crate::generation::height_limit::HeightLimitView;
use crate::generation::proto_chunk::GenerationCache;

/// Columns of the window around the centre chunk: the chunk and one column on every side.
const WINDOW: i32 = 18;

/// The part of the world whose light is made: its corner and the height of the cache.
#[derive(Clone, Copy)]
struct Window {
    start_x: i32,
    start_z: i32,
    bottom_y: i32,
    max_y: i32,
}

/// What a block takes from the light that goes through it.
fn opacity_of(state_id: BlockStateId) -> i32 {
    if state_id == BlockStateId::AIR {
        0
    } else {
        let s = state_id.to_state();
        if s.can_occlude() {
            s.opacity.max(1) as i32
        } else {
            s.opacity as i32
        }
    }
}

/// The light `(y, opacity)` pairs leave under a column of light 15, from the top down, until
/// none is left. Each item is the height and the light that stays there.
fn fade(heights_and_states: impl Iterator<Item = (i32, BlockStateId)>) -> Vec<(i32, u8)> {
    let mut out = Vec::new();
    let mut light: i32 = 15;
    for (y, state_id) in heights_and_states {
        light = light.saturating_sub(opacity_of(state_id));
        let light_val = if light <= 0 { 0 } else { light as u8 };
        out.push((y, light_val));
        if light <= 0 {
            break;
        }
    }
    out
}

impl SkyLightPropagator {
    pub fn convert_light(&mut self, cache: &mut Cache) {
        self.clear();

        let center_x = cache.x + (cache.size / 2);
        let center_z = cache.z + (cache.size / 2);
        let start_x = center_x * 16 - 1;
        let start_z = center_z * 16 - 1;
        let bottom_y = cache.bottom_y() as i32;
        let window = Window {
            start_x,
            start_z,
            bottom_y,
            max_y: bottom_y + cache.height() as i32,
        };

        let size = (cache.size * 16) as usize;
        self.visited.ensure_capacity(
            cache.x * 16,
            bottom_y,
            cache.z * 16,
            size,
            (window.max_y - bottom_y) as usize,
            size,
        );

        let surface_heights = Self::light_columns(cache, window);
        self.queue_light_entries(cache, window, &surface_heights);
        self.propagate(cache);
    }

    /// Gives every column of the window its light, and returns the height of each surface.
    fn light_columns(cache: &mut Cache, window: Window) -> [i32; (WINDOW * WINDOW) as usize] {
        let mut surface_heights = [0i32; (WINDOW * WINDOW) as usize];
        for z in window.start_z..window.start_z + WINDOW {
            let lz = (z - window.start_z) as usize;
            for x in window.start_x..window.start_x + WINDOW {
                let lx = (x - window.start_x) as usize;
                let top_y = cache.get_top_y(&HeightMap::WorldSurface, x, z);
                surface_heights[lx * WINDOW as usize + lz] = top_y;

                let rel_x = (x >> 4) - cache.x;
                let rel_z = (z >> 4) - cache.z;
                if rel_x < 0 || rel_x >= cache.size || rel_z < 0 || rel_z >= cache.size {
                    continue;
                }
                let chunk_idx = (rel_x * cache.size + rel_z) as usize;
                let (local_x, local_z) = ((x & 15) as usize, (z & 15) as usize);
                match &mut cache.chunks[chunk_idx] {
                    Chunk::Proto(c) => {
                        Self::light_proto_column(c, window, local_x, local_z, top_y);
                    }
                    Chunk::Level(c) => {
                        Self::light_level_column(c, window, local_x, local_z, top_y);
                    }
                }
            }
        }
        surface_heights
    }

    fn light_proto_column(
        c: &mut crate::ProtoChunk,
        window: Window,
        local_x: usize,
        local_z: usize,
        top_y: i32,
    ) {
        let bottom_y = window.bottom_y;
        let top_local_y = (top_y + 1 - bottom_y).max(0) as usize;
        let top_sec = top_local_y >> 4;
        let top_rem = top_local_y & 15;
        if top_sec < c.light.sky_light.len() {
            c.light.sky_light[top_sec].set_column_y_range(local_x, local_z, top_rem, 16, 15);
            for sec in (top_sec + 1)..c.light.sky_light.len() {
                c.light.sky_light[sec].set_column_y_range(local_x, local_z, 0, 16, 15);
            }
        }

        let states = (bottom_y..=top_y).rev().map(|y| {
            let state = c.get_block_state_raw(local_x as i32, y - bottom_y, local_z as i32);
            (y, state)
        });
        for (y, light_val) in fade(states.collect::<Vec<_>>().into_iter()) {
            let section_idx = ((y - bottom_y) >> 4) as usize;
            if section_idx < c.light.sky_light.len() {
                c.light.sky_light[section_idx].set(local_x, (y & 15) as usize, local_z, light_val);
            }
        }
    }

    fn light_level_column(
        c: &crate::chunk::ChunkData,
        window: Window,
        local_x: usize,
        local_z: usize,
        top_y: i32,
    ) {
        let bottom_y = window.bottom_y;
        let mut light_engine = c
            .light_engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for y in (top_y + 1)..window.max_y {
            let section_idx = ((y - bottom_y) >> 4) as usize;
            if section_idx < light_engine.sky_light.len() {
                light_engine.sky_light[section_idx].set(local_x, (y & 15) as usize, local_z, 15);
            }
        }

        let states = (bottom_y..=top_y).rev().map(|y| {
            let state = c
                .section
                .get_block_absolute_y(local_x, y, local_z)
                .unwrap_or(BlockStateId::AIR);
            (y, state)
        });
        for (y, light_val) in fade(states) {
            let section_idx = ((y - bottom_y) >> 4) as usize;
            if section_idx < light_engine.sky_light.len() {
                light_engine.sky_light[section_idx].set(
                    local_x,
                    (y & 15) as usize,
                    local_z,
                    light_val,
                );
            }
        }
    }

    /// Queues the lit cells that touch the surface or sit beside a taller neighbor column, so
    /// that the light enters the caves and overhangs there.
    fn queue_light_entries(
        &mut self,
        cache: &Cache,
        window: Window,
        surface_heights: &[i32; (WINDOW * WINDOW) as usize],
    ) {
        let width = WINDOW as usize;
        for z in window.start_z..window.start_z + WINDOW {
            let lz = (z - window.start_z) as usize;
            for x in window.start_x..window.start_x + WINDOW {
                let lx = (x - window.start_x) as usize;
                let top_y = surface_heights[lx * width + lz];

                let north_top = if lz > 0 {
                    surface_heights[lx * width + (lz - 1)]
                } else {
                    top_y
                };
                let south_top = if lz + 1 < width {
                    surface_heights[lx * width + (lz + 1)]
                } else {
                    top_y
                };
                let west_top = if lx > 0 {
                    surface_heights[(lx - 1) * width + lz]
                } else {
                    top_y
                };
                let east_top = if lx + 1 < width {
                    surface_heights[(lx + 1) * width + lz]
                } else {
                    top_y
                };

                let max_check_y = top_y
                    .max(north_top)
                    .max(south_top)
                    .max(west_top)
                    .max(east_top);

                for y in (window.bottom_y..=max_check_y).rev() {
                    let pos = BlockPos(Vector3::new(x, y, z));
                    let light = get_sky_light(cache, pos);

                    if light == 0 {
                        if y <= top_y {
                            break;
                        }
                        continue;
                    }

                    let is_at_surface = y == top_y;
                    let below_neighbor =
                        y < north_top || y < south_top || y < west_top || y < east_top;

                    if (is_at_surface || below_neighbor) && self.visited.test_and_set(x, y, z) {
                        let skip_dir = (y >= top_y).then_some(BlockDirection::Up);
                        self.queue.push_back(PropagationEntry {
                            pos,
                            skip_direction: skip_dir,
                        });
                    }
                }
            }
        }
    }
}
