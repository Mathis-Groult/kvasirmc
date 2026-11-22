use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_data::{Block, BlockId, BlockStateId, block_properties::is_air, tag};
use pumpkin_util::{
    math::vector3::Vector3,
    random::{RandomGenerator, RandomImpl},
};

use crate::generation::proto_chunk::GenerationCache;

pub struct IceSpikeFeature;

impl IceSpikeFeature {
    pub fn generate<T: GenerationCache>(
        chunk: &mut T,
        args: FeatureArgs<'_>,
        random: &mut RandomGenerator,
    ) -> bool {
        let FeatureArgs { pos, .. } = args;
        let Some(mut origin) = Self::find_snow_ground(chunk, pos.0) else {
            return false;
        };

        origin.y += random.next_bounded_i32(4);
        let height = random.next_bounded_i32(4) + 7;
        let width = height / 4 + random.next_bounded_i32(2);

        // Occasionally create extra-tall spires
        if width > 1 && random.next_bounded_i32(60) == 0 {
            origin.y += 10 + random.next_bounded_i32(30);
        }

        for y_off in 0..height {
            Self::place_layer(chunk, random, origin, y_off, height, width);
        }
        Self::place_roots(chunk, random, origin, width);

        true
    }

    /// Walks down while the block is air (staying above minY + 2) and returns the position when
    /// it is a snow block.
    fn find_snow_ground<T: GenerationCache>(
        chunk: &T,
        mut origin: Vector3<i32>,
    ) -> Option<Vector3<i32>> {
        while chunk.is_air(&origin) && origin.y > chunk.bottom_y() as i32 + 2 {
            origin.y -= 1;
        }

        let block_id = GenerationCache::get_block_state(chunk, &origin).to_block_id();
        (block_id == Block::SNOW_BLOCK.id).then_some(origin)
    }

    /// Air, dirt, snow block or ice: what the spike may replace.
    fn is_replaceable(state_id: BlockStateId) -> bool {
        let bid = state_id.to_block_id();
        is_air(state_id)
            || bid.has_tag(tag::Block::MINECRAFT_DIRT)
            || bid == BlockId::SNOW_BLOCK
            || bid == BlockId::ICE
    }

    /// Sets packed ice at `pos` when the block there is replaceable.
    fn replace_with_packed_ice<T: GenerationCache>(chunk: &mut T, pos: &Vector3<i32>) {
        let state_id = GenerationCache::get_block_state(chunk, pos);
        if Self::is_replaceable(state_id) {
            chunk.set_block_state(pos, Block::PACKED_ICE.default_state);
        }
    }

    /// One horizontal layer of the spike, `y_off` above the origin (and mirrored below it).
    fn place_layer<T: GenerationCache>(
        chunk: &mut T,
        random: &mut RandomGenerator,
        origin: Vector3<i32>,
        y_off: i32,
        height: i32,
        width: i32,
    ) {
        let scale = (1.0f32 - y_off as f32 / height as f32) * width as f32;
        let new_width = scale.ceil() as i32;

        for xo in -new_width..=new_width {
            let dx = xo.abs() as f32 - 0.25f32;

            for zo in -new_width..=new_width {
                let dz = zo.abs() as f32 - 0.25f32;

                let in_sphere = (xo == 0 && zo == 0) || (dx * dx + dz * dz <= scale * scale);
                let edge_ok =
                    (xo != -new_width && xo != new_width && zo != -new_width && zo != new_width)
                        || (random.next_f32() <= 0.75f32);

                if in_sphere && edge_ok {
                    // Place packed ice upward
                    let place_pos = origin.add(&Vector3::new(xo, y_off, zo));
                    Self::replace_with_packed_ice(chunk, &place_pos);

                    // Mirror downward
                    if y_off != 0 && new_width > 1 {
                        let neg_pos = origin.add(&Vector3::new(xo, -y_off, zo));
                        Self::replace_with_packed_ice(chunk, &neg_pos);
                    }
                }
            }
        }
    }

    /// Packed-ice roots downward from the spike base.
    fn place_roots<T: GenerationCache>(
        chunk: &mut T,
        random: &mut RandomGenerator,
        origin: Vector3<i32>,
        width: i32,
    ) {
        let pillar_width = (width - 1).clamp(0, 1);

        for xo in -pillar_width..=pillar_width {
            for zo in -pillar_width..=pillar_width {
                let ice_block = origin.add(&Vector3::new(xo, -1, zo));
                // Corner positions get a short random run; others run 50 blocks
                let run_length: i32 = if xo.abs() == 1 && zo.abs() == 1 {
                    random.next_bounded_i32(5)
                } else {
                    50
                };
                Self::place_root(chunk, random, ice_block, run_length);
            }
        }
    }

    /// One root pillar: packed ice down to y 50, stopping at the first block that is neither
    /// replaceable nor packed ice, skipping a random gap whenever the run length is used up.
    fn place_root<T: GenerationCache>(
        chunk: &mut T,
        random: &mut RandomGenerator,
        mut ice_block: Vector3<i32>,
        mut run_length: i32,
    ) {
        while ice_block.y > 50 {
            let state_id = GenerationCache::get_block_state(chunk, &ice_block);
            if !Self::is_replaceable(state_id) && state_id.to_block_id() != BlockId::PACKED_ICE {
                break;
            }

            chunk.set_block_state(&ice_block, Block::PACKED_ICE.default_state);
            ice_block.y -= 1;
            run_length -= 1;
            if run_length <= 0 {
                ice_block.y -= random.next_bounded_i32(5) + 1;
                run_length = random.next_bounded_i32(5);
            }
        }
    }
}

#[cfg(test)]
#[path = "ice_spike_golden.rs"]
mod golden;
