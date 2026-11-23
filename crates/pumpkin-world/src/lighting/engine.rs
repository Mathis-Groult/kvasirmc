use crate::chunk::format::LightContainer;
use crate::chunk_system::Chunk;
use crate::chunk_system::generation_cache::Cache;
use crate::generation::height_limit::HeightLimitView;
use crate::generation::proto_chunk::GenerationCache;
use crate::lighting::storage::{get_block_light, get_sky_light, set_block_light, set_sky_light};
use pumpkin_config::lighting::LightingEngineConfig;
use pumpkin_data::{BlockDirection, BlockState, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use std::collections::VecDeque;

use crate::ProtoChunk;

pub trait LightProvider {
    fn get_light(cache: &Cache, pos: BlockPos) -> u8;
    fn set_light(cache: &mut Cache, pos: BlockPos, level: u8);
    fn get_light_proto(
        chunk: &ProtoChunk,
        section_idx: usize,
        lx: usize,
        ly: usize,
        lz: usize,
    ) -> u8;
    fn set_light_proto(
        chunk: &mut ProtoChunk,
        section_idx: usize,
        lx: usize,
        ly: usize,
        lz: usize,
        level: u8,
    );
    fn propagate_level(current_level: u8, opacity: u8, dir: BlockDirection) -> u8;
}

pub struct BlockLightProvider;
impl LightProvider for BlockLightProvider {
    #[inline]
    fn get_light(cache: &Cache, pos: BlockPos) -> u8 {
        get_block_light(cache, pos)
    }
    #[inline]
    fn set_light(cache: &mut Cache, pos: BlockPos, level: u8) {
        set_block_light(cache, pos, level);
    }
    #[inline]
    fn get_light_proto(
        chunk: &ProtoChunk,
        section_idx: usize,
        lx: usize,
        ly: usize,
        lz: usize,
    ) -> u8 {
        chunk
            .light
            .block_light
            .get(section_idx)
            .map_or(0, |c| c.get(lx, ly, lz))
    }
    #[inline]
    fn set_light_proto(
        chunk: &mut ProtoChunk,
        section_idx: usize,
        lx: usize,
        ly: usize,
        lz: usize,
        level: u8,
    ) {
        if let Some(c) = chunk.light.block_light.get_mut(section_idx) {
            c.set(lx, ly, lz, level);
        }
    }
    #[inline]
    fn propagate_level(current_level: u8, opacity: u8, _dir: BlockDirection) -> u8 {
        current_level.saturating_sub(opacity.max(1))
    }
}

pub struct SkyLightProvider;
impl LightProvider for SkyLightProvider {
    #[inline]
    fn get_light(cache: &Cache, pos: BlockPos) -> u8 {
        get_sky_light(cache, pos)
    }
    #[inline]
    fn set_light(cache: &mut Cache, pos: BlockPos, level: u8) {
        set_sky_light(cache, pos, level);
    }
    #[inline]
    fn get_light_proto(
        chunk: &ProtoChunk,
        section_idx: usize,
        lx: usize,
        ly: usize,
        lz: usize,
    ) -> u8 {
        chunk
            .light
            .sky_light
            .get(section_idx)
            .map_or(15, |c| c.get(lx, ly, lz))
    }
    #[inline]
    fn set_light_proto(
        chunk: &mut ProtoChunk,
        section_idx: usize,
        lx: usize,
        ly: usize,
        lz: usize,
        level: u8,
    ) {
        if let Some(c) = chunk.light.sky_light.get_mut(section_idx) {
            c.set(lx, ly, lz, level);
        }
    }
    #[inline]
    fn propagate_level(current_level: u8, opacity: u8, dir: BlockDirection) -> u8 {
        if current_level == 15 && dir == BlockDirection::Down && opacity == 0 {
            return 15;
        }

        current_level.saturating_sub(opacity.max(1))
    }
}

#[derive(Clone, Copy)]
pub struct PropagationEntry {
    pub(super) pos: BlockPos,
    pub(super) skip_direction: Option<BlockDirection>,
}

pub struct VisitedBitSet {
    bits: Vec<u64>,
    min_x: i32,
    min_y: i32,
    min_z: i32,
    size_x: usize,
    size_y: usize,
    size_z: usize,
}

impl Default for VisitedBitSet {
    fn default() -> Self {
        Self::new()
    }
}

impl VisitedBitSet {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bits: Vec::new(),
            min_x: 0,
            min_y: 0,
            min_z: 0,
            size_x: 0,
            size_y: 0,
            size_z: 0,
        }
    }

    pub fn ensure_capacity(
        &mut self,
        min_x: i32,
        min_y: i32,
        min_z: i32,
        size_x: usize,
        size_y: usize,
        size_z: usize,
    ) {
        self.min_x = min_x;
        self.min_y = min_y;
        self.min_z = min_z;
        self.size_x = size_x;
        self.size_y = size_y;
        self.size_z = size_z;
        let total = size_x * size_y * size_z;
        let words = total.div_ceil(64);
        if self.bits.len() == words {
            self.bits.fill(0);
        } else {
            self.bits.resize(words, 0);
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    #[inline]
    pub fn test_and_set(&mut self, x: i32, y: i32, z: i32) -> bool {
        let lx = (x - self.min_x) as usize;
        let ly = (y - self.min_y) as usize;
        let lz = (z - self.min_z) as usize;
        if lx >= self.size_x || ly >= self.size_y || lz >= self.size_z {
            return false;
        }
        let idx = (ly * self.size_z + lz) * self.size_x + lx;
        let word = idx >> 6;
        let mask = 1u64 << (idx & 63);
        if let Some(w) = self.bits.get_mut(word) {
            let prev = *w;
            if prev & mask != 0 {
                return false;
            }
            *w = prev | mask;
            true
        } else {
            false
        }
    }

    #[inline]
    #[must_use]
    pub fn is_visited(&self, x: i32, y: i32, z: i32) -> bool {
        let lx = (x - self.min_x) as usize;
        let ly = (y - self.min_y) as usize;
        let lz = (z - self.min_z) as usize;
        if lx >= self.size_x || ly >= self.size_y || lz >= self.size_z {
            return true;
        }
        let idx = (ly * self.size_z + lz) * self.size_x + lx;
        let word = idx >> 6;
        let mask = 1u64 << (idx & 63);
        if let Some(&w) = self.bits.get(word) {
            (w & mask) != 0
        } else {
            true
        }
    }
}

/// Where a cache sits in the world, read once per `propagate`.
struct Frame {
    cache_x: i32,
    cache_z: i32,
    size: i32,
    min_y: i32,
    max_y: i32,
}

impl Frame {
    fn of(cache: &Cache) -> Self {
        let min_y = cache.bottom_y() as i32;
        Self {
            cache_x: cache.x,
            cache_z: cache.z,
            size: cache.size,
            min_y,
            max_y: min_y + cache.height() as i32,
        }
    }
}

/// A block position inside the cache, split into the chunk it is in and the place in that chunk.
struct Cell {
    pos: BlockPos,
    chunk_idx: usize,
    section_idx: usize,
    local_x: usize,
    local_y: usize,
    local_z: usize,
}

impl Cell {
    /// `pos` must be inside the cache.
    const fn at(frame: &Frame, pos: BlockPos) -> Self {
        let Vector3 { x, y, z } = pos.0;
        let rel_x = (x >> 4) - frame.cache_x;
        let rel_z = (z >> 4) - frame.cache_z;
        Self {
            pos,
            chunk_idx: (rel_x * frame.size + rel_z) as usize,
            section_idx: ((y - frame.min_y) >> 4) as usize,
            local_x: (x & 15) as usize,
            local_y: (y & 15) as usize,
            local_z: (z & 15) as usize,
        }
    }

    /// The light opacity of the block here and the light it has now.
    fn read<P: LightProvider>(&self, cache: &Cache) -> (u8, u8) {
        match &cache.chunks[self.chunk_idx] {
            Chunk::Proto(c) => {
                let state_id = c.get_block_state_raw(
                    self.local_x as i32,
                    self.pos.0.y - cache.bottom_y() as i32,
                    self.local_z as i32,
                );
                let light = P::get_light_proto(
                    c,
                    self.section_idx,
                    self.local_x,
                    self.local_y,
                    self.local_z,
                );
                (light_opacity(state_id), light)
            }
            Chunk::Level(lvl) => {
                let state_id = lvl
                    .section
                    .get_block_absolute_y(self.local_x, self.pos.0.y, self.local_z)
                    .unwrap_or(BlockStateId::AIR);
                (light_opacity(state_id), P::get_light(cache, self.pos))
            }
        }
    }

    fn write<P: LightProvider>(&self, cache: &mut Cache, level: u8) {
        match &mut cache.chunks[self.chunk_idx] {
            Chunk::Proto(c) => P::set_light_proto(
                c,
                self.section_idx,
                self.local_x,
                self.local_y,
                self.local_z,
                level,
            ),
            Chunk::Level(_) => P::set_light(cache, self.pos, level),
        }
    }
}

/// How much light the block of `state_id` takes out of what passes through it.
fn light_opacity(state_id: BlockStateId) -> u8 {
    if state_id == BlockStateId::AIR {
        return 0;
    }
    let state = state_id.to_state();
    if state.can_occlude() {
        state.opacity.max(1)
    } else {
        state.opacity
    }
}

pub struct LightPropagator<P: LightProvider> {
    pub(crate) queue: VecDeque<PropagationEntry>,
    pub(crate) visited: VisitedBitSet,
    pub(crate) decrease_queue: VecDeque<(BlockPos, u8)>,
    _marker: std::marker::PhantomData<P>,
}

impl<P: LightProvider> LightPropagator<P> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(8192),
            visited: VisitedBitSet::new(),
            decrease_queue: VecDeque::new(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn clear(&mut self) {
        self.queue.clear();
        self.visited.clear();
        self.decrease_queue.clear();
    }

    /// Spreads the light of every queued cell to its neighbours, until the queue is empty.
    pub fn propagate(&mut self, cache: &mut Cache) {
        let frame = Frame::of(cache);
        self.visited.ensure_capacity(
            frame.cache_x * 16,
            frame.min_y,
            frame.cache_z * 16,
            (frame.size * 16) as usize,
            (frame.max_y - frame.min_y) as usize,
            (frame.size * 16) as usize,
        );

        while let Some(entry) = self.queue.pop_front() {
            let current_light = P::get_light(cache, entry.pos);
            if current_light <= 1 {
                continue;
            }

            for dir in BlockDirection::all() {
                if entry.skip_direction == Some(dir) {
                    continue;
                }
                self.spread(cache, &frame, entry.pos, current_light, dir);
            }
        }
    }

    /// One step of `propagate`: lights the neighbour of `pos` in direction `dir` when the light
    /// that reaches it is brighter than what it has, and queues it. A neighbour outside the cache
    /// counts as visited, so it is never reached here.
    fn spread(
        &mut self,
        cache: &mut Cache,
        frame: &Frame,
        pos: BlockPos,
        current_light: u8,
        dir: BlockDirection,
    ) {
        let neighbor_pos = pos.offset(dir.to_offset());
        let Vector3 { x, y, z } = neighbor_pos.0;
        if self.visited.is_visited(x, y, z) {
            return;
        }

        let cell = Cell::at(frame, neighbor_pos);
        let (opacity, neighbor_light) = cell.read::<P>(cache);
        let new_level = P::propagate_level(current_light, opacity, dir);
        if new_level <= neighbor_light {
            return;
        }

        cell.write::<P>(cache, new_level);
        if new_level > 1 && self.visited.test_and_set(x, y, z) {
            self.queue.push_back(PropagationEntry {
                pos: neighbor_pos,
                skip_direction: Some(dir.opposite()),
            });
        }
    }

    pub fn process_decrease_queue(&mut self, cache: &mut Cache) {
        let cache_x = cache.x;
        let cache_z = cache.z;
        let cache_size = cache.size;

        while let Some((pos, old_val)) = self.decrease_queue.pop_front() {
            for dir in BlockDirection::all() {
                let neighbor_pos = pos.offset(dir.to_offset());

                let (cx, _rel) = neighbor_pos.chunk_and_chunk_relative_position();
                let rel_x = cx.x - cache_x;
                let rel_z = cx.y - cache_z;

                if rel_x < 0 || rel_x >= cache_size || rel_z < 0 || rel_z >= cache_size {
                    continue;
                }

                let neighbor_light = P::get_light(cache, neighbor_pos);
                if neighbor_light == 0 {
                    continue;
                }

                let state = cache.get_block_state(&neighbor_pos.0).to_state();
                let opacity = if state.can_occlude() {
                    state.opacity.max(1)
                } else {
                    state.opacity
                };

                let predicted = P::propagate_level(old_val, opacity, dir);

                if neighbor_light == predicted || neighbor_light < old_val {
                    P::set_light(cache, neighbor_pos, 0);
                    self.decrease_queue
                        .push_back((neighbor_pos, neighbor_light));
                } else if neighbor_light >= old_val {
                    let nx = neighbor_pos.0.x;
                    let ny = neighbor_pos.0.y;
                    let nz = neighbor_pos.0.z;
                    self.queue.push_back(PropagationEntry {
                        pos: neighbor_pos,
                        skip_direction: None,
                    });
                    self.visited.test_and_set(nx, ny, nz);
                }
            }
        }

        self.propagate(cache);
    }
}

pub type BlockLightPropagator = LightPropagator<BlockLightProvider>;
pub type SkyLightPropagator = LightPropagator<SkyLightProvider>;

impl<P: LightProvider> Default for LightPropagator<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockLightPropagator {
    pub fn propagate_light(&mut self, cache: &mut Cache) {
        self.clear();

        let min_y = cache.bottom_y() as i32;
        let max_y = min_y + cache.height() as i32;
        let center_x = cache.x + (cache.size / 2);
        let center_z = cache.z + (cache.size / 2);

        let start_x = center_x * 16 - 1;
        let start_z = center_z * 16 - 1;
        let end_x = start_x + 18;
        let end_z = start_z + 18;

        let min_x = cache.x * 16;
        let min_z = cache.z * 16;
        let size_x = (cache.size * 16) as usize;
        let size_z = (cache.size * 16) as usize;
        let size_y = (max_y - min_y) as usize;
        self.visited
            .ensure_capacity(min_x, min_y, min_z, size_x, size_y, size_z);

        for z in start_z..end_z {
            let rel_z = (z >> 4) - cache.z;
            let local_z = (z & 15) as usize;

            for x in start_x..end_x {
                let rel_x = (x >> 4) - cache.x;
                if rel_x < 0 || rel_x >= cache.size || rel_z < 0 || rel_z >= cache.size {
                    continue;
                }
                let chunk_idx = (rel_x * cache.size + rel_z) as usize;
                let local_x = (x & 15) as usize;

                match &mut cache.chunks[chunk_idx] {
                    Chunk::Proto(c) => {
                        for y in min_y..max_y {
                            let local_y_proto = y - min_y;
                            let state_id = c.get_block_state_raw(
                                local_x as i32,
                                local_y_proto,
                                local_z as i32,
                            );
                            if state_id == BlockStateId::AIR {
                                continue;
                            }
                            let emission = state_id.to_state().luminance;
                            if emission > 0 {
                                let section_idx = (local_y_proto >> 4) as usize;
                                let local_y = (y & 15) as usize;
                                if section_idx < c.light.block_light.len() {
                                    c.light.block_light[section_idx]
                                        .set(local_x, local_y, local_z, emission);
                                }
                                if self.visited.test_and_set(x, y, z) {
                                    let pos = BlockPos(Vector3::new(x, y, z));
                                    self.queue.push_back(PropagationEntry {
                                        pos,
                                        skip_direction: None,
                                    });
                                }
                            }
                        }
                    }
                    Chunk::Level(lvl) => {
                        let mut light_engine = lvl
                            .light_engine
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        for y in min_y..max_y {
                            let state_id = lvl
                                .section
                                .get_block_absolute_y(local_x, y, local_z)
                                .unwrap_or(BlockStateId::AIR);
                            if state_id == BlockStateId::AIR {
                                continue;
                            }
                            let emission = state_id.to_state().luminance;
                            if emission > 0 {
                                let section_idx = ((y - min_y) >> 4) as usize;
                                let local_y = (y & 15) as usize;
                                if section_idx < light_engine.block_light.len() {
                                    light_engine.block_light[section_idx]
                                        .set(local_x, local_y, local_z, emission);
                                }
                                if self.visited.test_and_set(x, y, z) {
                                    let pos = BlockPos(Vector3::new(x, y, z));
                                    self.queue.push_back(PropagationEntry {
                                        pos,
                                        skip_direction: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        self.propagate(cache);
    }
}

pub struct LightEngine {
    block_light: BlockLightPropagator,
    sky_light: SkyLightPropagator,
}

impl LightEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            block_light: BlockLightPropagator::new(),
            sky_light: SkyLightPropagator::new(),
        }
    }

    pub fn initialize_light(
        &mut self,
        cache: &mut Cache,
        config: &LightingEngineConfig,
        has_skylight: bool,
    ) {
        if *config != LightingEngineConfig::Default {
            return;
        }

        let should_skip = {
            let center_chunk = cache.get_center_chunk();
            center_chunk.stage >= crate::chunk_system::chunk_state::StagedChunkEnum::Lighting
        };
        if should_skip {
            return;
        }

        if has_skylight {
            self.sky_light.convert_light(cache);
        } else {
            for section in &mut cache.get_center_chunk_mut().light.sky_light {
                *section = LightContainer::new_empty(0);
            }
        }
        self.block_light.propagate_light(cache);

        self.block_light.clear();
        self.sky_light.clear();
    }

    pub fn update_block_light(
        &mut self,
        cache: &mut Cache,
        pos: BlockPos,
        old_luminance: u8,
        new_luminance: u8,
    ) {
        if old_luminance > new_luminance {
            let current_light = get_block_light(cache, pos);
            if current_light > 0 {
                self.block_light
                    .decrease_queue
                    .push_back((pos, current_light));
                set_block_light(cache, pos, 0);
            }
        }

        if new_luminance > 0 {
            set_block_light(cache, pos, new_luminance);
            if self
                .block_light
                .visited
                .test_and_set(pos.0.x, pos.0.y, pos.0.z)
            {
                self.block_light.queue.push_back(PropagationEntry {
                    pos,
                    skip_direction: None,
                });
            }
        }
    }

    pub fn run_light_updates(&mut self, cache: &mut Cache) {
        if !self.block_light.decrease_queue.is_empty() {
            self.block_light.process_decrease_queue(cache);
        }
        if !self.block_light.queue.is_empty() {
            self.block_light.propagate(cache);
            self.block_light.visited.clear();
        }
        if !self.sky_light.decrease_queue.is_empty() {
            self.sky_light.process_decrease_queue(cache);
        }
        if !self.sky_light.queue.is_empty() {
            self.sky_light.propagate(cache);
            self.sky_light.visited.clear();
        }
    }

    /// Checks if a block state has an empty shape for light occlusion, matching vanilla `LightEngine.isEmptyShape`.
    #[inline]
    #[must_use]
    pub const fn is_empty_shape(state: &BlockState) -> bool {
        !state.can_occlude()
    }

    /// Checks if two block states have different light properties, matching vanilla `LightEngine.hasDifferentLightProperties`.
    #[inline]
    #[must_use]
    pub fn has_different_light_properties(old_state: &BlockState, new_state: &BlockState) -> bool {
        if std::ptr::eq(old_state, new_state) || old_state.id == new_state.id {
            return false;
        }

        old_state.opacity != new_state.opacity
            || old_state.luminance != new_state.luminance
            || old_state.can_occlude() != new_state.can_occlude()
    }

    /// Returns the minimum light dampening for this block state, matching vanilla `LightEngine.getOpacity`.
    #[inline]
    #[must_use]
    pub const fn get_opacity(state: &BlockState) -> u8 {
        if state.opacity > 1 { state.opacity } else { 1 }
    }

    /// Calculates the light dampening between two blocks, matching vanilla `LightEngine.getLightDampeningInto`.
    #[inline]
    #[must_use]
    pub const fn get_light_dampening_into(
        from_state: &BlockState,
        to_state: &BlockState,
        _direction: BlockDirection,
        simple_opacity: u8,
    ) -> u8 {
        let from_empty = Self::is_empty_shape(from_state);
        let to_empty = Self::is_empty_shape(to_state);
        if from_empty && to_empty {
            return simple_opacity;
        }
        if to_state.can_occlude() && to_state.is_solid_render() {
            return 16;
        }
        simple_opacity
    }
}

impl Default for LightEngine {
    fn default() -> Self {
        Self::new()
    }
}
