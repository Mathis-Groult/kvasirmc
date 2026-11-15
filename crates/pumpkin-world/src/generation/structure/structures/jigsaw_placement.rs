use pumpkin_data::{Mirror, Rotation};

use crate::generation::structure::{
    piece::StructurePieceType,
    structures::{HeightSampler, StructureGeneratorContext, StructurePiece, StructurePosition},
};
use pumpkin_util::math::block_box::BlockBox;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::random::{RandomGenerator, RandomImpl};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use super::jigsaw::{
    JigsawBlock, JigsawJointType, JigsawJunction, JigsawProjection, PoolElement,
    PoolElementStructurePiece, TemplatePool,
};

pub struct JigsawPlacement;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DimensionPadding {
    pub top: i32,
    pub bottom: i32,
}

impl DimensionPadding {
    pub const ZERO: Self = Self { top: 0, bottom: 0 };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LiquidSettings {
    IgnoreWaterlogDone,
    ApplyWaterlog,
}

pub struct MaxDistance {
    pub horizontal: i32,
    pub vertical: i32,
}

impl MaxDistance {
    #[must_use]
    pub const fn new(horizontal: i32) -> Self {
        Self {
            horizontal,
            vertical: 384, // Default Y_SIZE (min_y to max_y)
        }
    }
}

pub const MAX_TOTAL_STRUCTURE_RANGE: i32 = 128;
pub const MIN_DEPTH: i32 = 0;
pub const MAX_DEPTH: i32 = 20;

/// Dynamic lookup for Pool Aliases introduced in 1.20+ (e.g. Trial Chambers spawner/contents aliases).
#[derive(Debug, Clone, Default)]
pub struct PoolAliasLookup {
    aliases: HashMap<String, String>,
}

impl PoolAliasLookup {
    #[must_use]
    pub fn new() -> Self {
        Self {
            aliases: HashMap::new(),
        }
    }

    #[must_use]
    pub fn from_bindings(
        bindings: &[pumpkin_data::structures::PoolAliasBinding],
        random: &mut RandomGenerator,
    ) -> Self {
        let mut lookup = Self::new();
        for binding in bindings {
            lookup.add_binding(binding, random);
        }
        lookup
    }

    fn add_binding(
        &mut self,
        binding: &pumpkin_data::structures::PoolAliasBinding,
        random: &mut RandomGenerator,
    ) {
        use pumpkin_data::structures::PoolAliasBinding;
        match binding {
            PoolAliasBinding::Direct { alias, target } => {
                self.add_direct((*alias).to_string(), (*target).to_string());
            }
            PoolAliasBinding::Random { alias, targets } => {
                if let Some(target) = Self::pick_weighted(targets, random, |t| t.weight) {
                    self.add_direct((*alias).to_string(), target.target.to_string());
                }
            }
            PoolAliasBinding::RandomGroup { groups } => {
                if let Some(group) = Self::pick_weighted(groups, random, |g| g.weight) {
                    for b in group.bindings {
                        self.add_binding(b, random);
                    }
                }
            }
        }
    }

    fn pick_weighted<'a, T>(
        items: &'a [T],
        random: &mut RandomGenerator,
        get_weight: impl Fn(&T) -> u32,
    ) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let total_weight: u32 = items.iter().map(&get_weight).sum();
        if total_weight == 0 {
            return items.first();
        }
        let mut r = random.next_bounded_i32(total_weight as i32) as u32;
        for item in items {
            let w = get_weight(item);
            if r < w {
                return Some(item);
            }
            r -= w;
        }
        items.first()
    }

    pub fn add_direct(&mut self, alias: String, target: String) {
        if let Some(stripped) = alias.strip_prefix("minecraft:") {
            self.aliases.insert(stripped.to_string(), target.clone());
        } else {
            self.aliases
                .insert(format!("minecraft:{alias}"), target.clone());
        }
        self.aliases.insert(alias, target);
    }

    pub fn add_random(
        &mut self,
        alias: String,
        targets: &[(String, u32)],
        random: &mut RandomGenerator,
    ) {
        if targets.is_empty() {
            return;
        }
        let total_weight: u32 = targets.iter().map(|(_, w)| *w).sum();
        if total_weight == 0 {
            if let Some((target, _)) = targets.first() {
                self.add_direct(alias, target.clone());
            }
            return;
        }
        let mut r = random.next_bounded_i32(total_weight as i32) as u32;
        for (target, weight) in targets {
            if r < *weight {
                self.add_direct(alias, target.clone());
                return;
            }
            r -= *weight;
        }
        if let Some((target, _)) = targets.first() {
            self.add_direct(alias, target.clone());
        }
    }

    #[must_use]
    pub fn lookup<'a>(&'a self, mut id: &'a str, _random: &mut RandomGenerator) -> &'a str {
        while let Some(target) = self.aliases.get(id) {
            id = target.as_str();
        }
        id
    }
}

#[derive(Clone, Debug)]
pub struct FreeSpace {
    pub bounds: BlockBox,
    pub occupied: Vec<BlockBox>,
}

impl FreeSpace {
    #[must_use]
    pub fn new(bounds: BlockBox, center_box: BlockBox) -> Self {
        Self {
            bounds,
            occupied: vec![center_box],
        }
    }

    #[must_use]
    pub const fn from_bounds(bounds: BlockBox) -> Self {
        Self {
            bounds,
            occupied: Vec::new(),
        }
    }

    #[must_use]
    pub fn can_fit(&self, candidate: &BlockBox) -> bool {
        is_box_inside(&self.bounds, candidate)
            && !self.occupied.iter().any(|b| boxes_intersect(b, candidate))
    }

    pub fn occupy(&mut self, bbox: BlockBox) {
        self.occupied.push(bbox);
    }
}

struct PieceState {
    piece_idx: usize,
    free: Rc<RefCell<FreeSpace>>,
    depth: i32,
}

struct PlacingQueue {
    queues: BTreeMap<std::cmp::Reverse<i32>, VecDeque<PieceState>>,
}

impl PlacingQueue {
    const fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
        }
    }

    fn add(&mut self, state: PieceState, priority: i32) {
        self.queues
            .entry(std::cmp::Reverse(priority))
            .or_default()
            .push_back(state);
    }

    fn next(&mut self) -> Option<PieceState> {
        let mut empty_key = None;
        let mut result = None;
        for (&priority_rev, queue) in &mut self.queues {
            if let Some(state) = queue.pop_front() {
                result = Some(state);
                if queue.is_empty() {
                    empty_key = Some(priority_rev);
                }
                break;
            }
        }
        if let Some(key) = empty_key {
            self.queues.remove(&key);
        }
        result
    }

    fn has_next(&self) -> bool {
        self.queues.values().any(|q| !q.is_empty())
    }
}

#[allow(clippy::vec_box)]
struct Placer {
    max_depth: i32,
    pieces: Vec<Box<PoolElementStructurePiece>>,
    placing: PlacingQueue,
}

impl Placer {
    #[allow(clippy::vec_box)]
    fn new(max_depth: i32, center_piece: Box<PoolElementStructurePiece>) -> Self {
        Self {
            max_depth,
            pieces: vec![center_piece],
            placing: PlacingQueue::new(),
        }
    }

    /// Places the center piece's children, then theirs in priority order, and adds every piece
    /// to the collector.
    fn place_all(
        max_depth: i32,
        center_piece: Box<PoolElementStructurePiece>,
        global_free: Rc<RefCell<FreeSpace>>,
        run: &mut Run<'_, '_, '_>,
        collector: &mut super::StructurePiecesCollector,
    ) {
        let mut placer = Self::new(max_depth, center_piece);
        placer.try_placing_children(0, global_free, 0, run);

        while placer.placing.has_next() {
            if let Some(state) = placer.placing.next() {
                placer.try_placing_children(state.piece_idx, state.free, state.depth, run);
            }
        }
        for piece in placer.pieces {
            collector.add_piece(piece);
        }
    }

    #[expect(clippy::needless_pass_by_value)]
    fn try_placing_children(
        &mut self,
        source_piece_idx: usize,
        context_free: Rc<RefCell<FreeSpace>>,
        depth: i32,
        run: &mut Run<'_, '_, '_>,
    ) {
        let source_element = self.pieces[source_piece_idx].element.clone();
        let source_box_position = self.pieces[source_piece_idx].pos;
        let source_rotation = self.pieces[source_piece_idx].rotation;
        let source_projection = source_element.projection;
        let source_rigid = source_projection == JigsawProjection::Rigid;
        let mut source_free: Option<Rc<RefCell<FreeSpace>>> = None;
        let source_bb = self.pieces[source_piece_idx].piece.bounding_box;
        let source_box_y = source_bb.min.y;

        let source_jigsaws = source_element.get_shuffled_jigsaw_blocks(
            source_box_position,
            source_rotation,
            run.random,
        );
        let source = SourcePiece {
            idx: source_piece_idx,
            bb: source_bb,
            box_y: source_box_y,
            rigid: source_rigid,
            projection: source_projection,
            depth,
        };

        for source_jigsaw in source_jigsaws {
            self.place_from_jigsaw(&source, &mut source_free, &context_free, source_jigsaw, run);
        }
    }

    /// Attaches the first child that fits to one jigsaw of the source piece, trying the jigsaw's
    /// pool, then its fallback pool.
    fn place_from_jigsaw(
        &mut self,
        source: &SourcePiece,
        source_free: &mut Option<Rc<RefCell<FreeSpace>>>,
        context_free: &Rc<RefCell<FreeSpace>>,
        source_jigsaw: JigsawBlock,
        run: &mut Run<'_, '_, '_>,
    ) {
        let source_direction = source_jigsaw.facing;
        let source_jigsaw_pos = source_jigsaw.pos;
        let target_jigsaw_pos = source_jigsaw_pos.add(
            source_direction.to_vector().x,
            source_direction.to_vector().y,
            source_direction.to_vector().z,
        );
        let source_jigsaw_local_y = source_jigsaw_pos.0.y - source.box_y;

        let pool_name = run
            .pool_alias_lookup
            .lookup(&source_jigsaw.pool, run.random);
        if pool_name == "minecraft:empty" || pool_name.is_empty() {
            return;
        }
        let Some(target_pool) = TemplatePool::discover(pool_name) else {
            tracing::warn!("Empty or non-existent pool: {}", pool_name);
            return;
        };

        if target_pool.elements.is_empty() && target_pool.id != "minecraft:empty" {
            tracing::warn!("Empty or non-existent pool: {}", pool_name);
            return;
        }

        let fallback_pool_name = run
            .pool_alias_lookup
            .lookup(&target_pool.fallback, run.random);
        let fallback_pool = TemplatePool::discover(fallback_pool_name);

        let attach_inside_source = source.bb.contains(
            target_jigsaw_pos.0.x,
            target_jigsaw_pos.0.y,
            target_jigsaw_pos.0.z,
        );
        let children_free = if attach_inside_source {
            source_free
                .get_or_insert_with(|| Rc::new(RefCell::new(FreeSpace::from_bounds(source.bb))))
                .clone()
        } else {
            context_free.clone()
        };

        let mut target_pieces = Vec::new();
        if source.depth != self.max_depth {
            target_pieces.extend(target_pool.get_shuffled_elements(run.random));
        }
        if let Some(fb) = &fallback_pool {
            target_pieces.extend(fb.get_shuffled_elements(run.random));
        }

        let mut source_jigsaw = SourceJigsaw {
            jigsaw: source_jigsaw,
            target_pos: target_jigsaw_pos,
            local_y: source_jigsaw_local_y,
            base_height: i32::MIN,
            children_free,
        };

        for target_element in target_pieces {
            if target_element.is_empty() {
                break;
            }
            if self.try_element(source, &mut source_jigsaw, target_element, run) {
                return;
            }
        }
    }

    /// Tries the element in its four rotations (in random order) and with each of its jigsaws
    /// that can attach; adds it and returns true at the first box that fits.
    fn try_element(
        &mut self,
        source: &SourcePiece,
        source_jigsaw: &mut SourceJigsaw,
        target_element: Arc<PoolElement>,
        run: &mut Run<'_, '_, '_>,
    ) -> bool {
        let mut rotations = Rotation::values();
        for i in (1..4).rev() {
            let j = run.random.next_bounded_i32(i as i32 + 1) as usize;
            rotations.swap(i, j);
        }

        for target_rotation in rotations {
            let target_jigsaws = target_element.get_shuffled_jigsaw_blocks(
                BlockPos::ZERO,
                target_rotation,
                run.random,
            );
            let hack_box = target_element.get_bounding_box(BlockPos::ZERO, target_rotation);
            let hack_box_y_span = hack_box.max.y - hack_box.min.y + 1;

            let expand_to = if run.do_expansion_hack && hack_box_y_span <= 16 {
                expansion_height(
                    &target_jigsaws,
                    &hack_box,
                    run.pool_alias_lookup,
                    run.random,
                )
            } else {
                0
            };

            for target_jigsaw in target_jigsaws {
                if can_attach(&source_jigsaw.jigsaw, &target_jigsaw) {
                    let target_jigsaw_local_pos = target_jigsaw.pos;
                    let raw_target_box_pos = BlockPos::new(
                        source_jigsaw.target_pos.0.x - target_jigsaw_local_pos.0.x,
                        source_jigsaw.target_pos.0.y - target_jigsaw_local_pos.0.y,
                        source_jigsaw.target_pos.0.z - target_jigsaw_local_pos.0.z,
                    );
                    let raw_target_bb =
                        target_element.get_bounding_box(raw_target_box_pos, target_rotation);
                    let raw_target_y = raw_target_bb.min.y;
                    let target_projection = target_element.projection;
                    let target_rigid = target_projection == JigsawProjection::Rigid;
                    let target_jigsaw_local_y = target_jigsaw_local_pos.0.y;
                    let delta_y = source_jigsaw.local_y - target_jigsaw_local_y
                        + source_jigsaw.jigsaw.facing.to_vector().y;

                    let target_box_y = if source.rigid && target_rigid {
                        source.box_y + delta_y
                    } else {
                        source_jigsaw.base_height(run.height_sampler) - target_jigsaw_local_y
                    };

                    let y_offset = target_box_y - raw_target_y;
                    let mut target_bb = raw_target_bb;
                    target_bb.move_pos(0, y_offset, 0);
                    let target_box_position = raw_target_box_pos.add(0, y_offset, 0);

                    if expand_to > 0 {
                        let new_size = (expand_to + 1).max(target_bb.max.y - target_bb.min.y);
                        target_bb.max.y = target_bb.min.y + new_size;
                    }

                    if source_jigsaw.children_free.borrow().can_fit(&target_bb) {
                        let child = Child {
                            bb: target_bb,
                            pos: target_box_position,
                            rotation: target_rotation,
                            box_y: target_box_y,
                            jigsaw_local_y: target_jigsaw_local_y,
                            delta_y,
                            rigid: target_rigid,
                            projection: target_projection,
                        };
                        self.add_child(source, source_jigsaw, target_element, &child, run);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Adds a child that fits: occupies its box, joins it to the source piece with a junction on
    /// each side, and queues it for its own children unless it is at the maximum depth.
    fn add_child(
        &mut self,
        source: &SourcePiece,
        source_jigsaw: &mut SourceJigsaw,
        target_element: Arc<PoolElement>,
        child: &Child,
        run: &mut Run<'_, '_, '_>,
    ) {
        source_jigsaw.children_free.borrow_mut().occupy(child.bb);

        let source_ground_level_delta = self.pieces[source.idx].ground_level_delta;
        let target_ground_level_delta = if child.rigid {
            source_ground_level_delta - child.delta_y
        } else {
            target_element.get_ground_level_delta()
        };

        let mut target_piece = Box::new(PoolElementStructurePiece {
            piece: StructurePiece::new(
                StructurePieceType::Jigsaw,
                child.bb,
                source.depth as u32 + 1,
            ),
            element: target_element,
            pos: child.pos,
            rotation: child.rotation,
            mirror: Mirror::None,
            jigsaw_blocks: Vec::new(),
            junctions: Vec::new(),
            ground_level_delta: target_ground_level_delta,
            liquid_settings: run.liquid_settings,
            projection: child.projection,
        });

        let junction_y = if source.rigid {
            source.box_y + source_jigsaw.local_y
        } else if child.rigid {
            child.box_y + child.jigsaw_local_y
        } else {
            source_jigsaw.base_height(run.height_sampler) + child.delta_y / 2
        };

        self.pieces[source.idx].add_junction(JigsawJunction {
            source_x: source_jigsaw.target_pos.0.x,
            source_ground_y: junction_y - source_jigsaw.local_y + source_ground_level_delta,
            source_z: source_jigsaw.target_pos.0.z,
            delta_y: child.delta_y,
            projection: child.projection,
        });

        target_piece.add_junction(JigsawJunction {
            source_x: source_jigsaw.jigsaw.pos.0.x,
            source_ground_y: junction_y - child.jigsaw_local_y + target_ground_level_delta,
            source_z: source_jigsaw.jigsaw.pos.0.z,
            delta_y: -child.delta_y,
            projection: source.projection,
        });

        let target_piece_idx = self.pieces.len();
        self.pieces.push(target_piece);

        if source.depth < self.max_depth {
            self.placing.add(
                PieceState {
                    piece_idx: target_piece_idx,
                    free: source_jigsaw.children_free.clone(),
                    depth: source.depth + 1,
                },
                source_jigsaw.jigsaw.placement_priority,
            );
        }
    }
}

/// What every placement of one structure shares.
struct Run<'r, 's, 'h> {
    do_expansion_hack: bool,
    height_sampler: &'r mut Option<&'s mut (dyn HeightSampler + 'h)>,
    random: &'r mut RandomGenerator,
    pool_alias_lookup: &'r PoolAliasLookup,
    liquid_settings: LiquidSettings,
}

/// The piece whose jigsaws are being connected.
struct SourcePiece {
    idx: usize,
    bb: BlockBox,
    box_y: i32,
    rigid: bool,
    projection: JigsawProjection,
    depth: i32,
}

/// One jigsaw of the source piece: where a child's jigsaw has to sit to face it, the free space
/// its children go in, and the ground height at it, sampled at most once.
struct SourceJigsaw {
    jigsaw: JigsawBlock,
    target_pos: BlockPos,
    local_y: i32,
    base_height: i32,
    children_free: Rc<RefCell<FreeSpace>>,
}

impl SourceJigsaw {
    fn base_height(&mut self, height_sampler: &mut Option<&mut (dyn HeightSampler + '_)>) -> i32 {
        if self.base_height == i32::MIN {
            self.base_height = height_sampler.as_mut().map_or(self.jigsaw.pos.0.y, |s| {
                s.estimate_height(self.jigsaw.pos.0.x, self.jigsaw.pos.0.z)
            });
        }
        self.base_height
    }
}

/// Where a child piece goes once one of its jigsaws faces the source jigsaw.
struct Child {
    bb: BlockBox,
    pos: BlockPos,
    rotation: Rotation,
    box_y: i32,
    jigsaw_local_y: i32,
    delta_y: i32,
    rigid: bool,
    projection: JigsawProjection,
}

/// With the expansion hack, the height a child's box is stretched to: the largest pool reachable
/// from a jigsaw that faces into the child.
fn expansion_height(
    target_jigsaws: &[JigsawBlock],
    hack_box: &BlockBox,
    pool_alias_lookup: &PoolAliasLookup,
    random: &mut RandomGenerator,
) -> i32 {
    let mut expand_to = 0;
    for target_jigsaw_x in target_jigsaws {
        let target_jigsaw_relative = target_jigsaw_x.pos.add(
            target_jigsaw_x.facing.to_vector().x,
            target_jigsaw_x.facing.to_vector().y,
            target_jigsaw_x.facing.to_vector().z,
        );
        if !hack_box.contains(
            target_jigsaw_relative.0.x,
            target_jigsaw_relative.0.y,
            target_jigsaw_relative.0.z,
        ) {
            continue;
        }

        let child_pool_name = pool_alias_lookup.lookup(&target_jigsaw_x.pool, random);
        let child_pool = TemplatePool::discover(child_pool_name);
        let child_pool_size = child_pool.as_ref().map_or(0, |p| p.get_max_size());
        let child_fallback_size = child_pool
            .as_ref()
            .and_then(|p| {
                let fb_name = pool_alias_lookup.lookup(&p.fallback, random);
                TemplatePool::discover(fb_name)
            })
            .map_or(0, |p| p.get_max_size());

        expand_to = expand_to.max(child_pool_size.max(child_fallback_size));
    }
    expand_to
}

impl JigsawPlacement {
    #[expect(clippy::too_many_arguments)]
    pub fn add_pieces(
        context: &mut StructureGeneratorContext,
        start_pool_id: &str,
        start_jigsaw: Option<&str>,
        max_depth: i32,
        position: BlockPos,
        do_expansion_hack: bool,
        project_start_to_heightmap: bool,
        max_distance_from_center: &MaxDistance,
        dimension_padding: DimensionPadding,
        liquid_settings: LiquidSettings,
        pool_alias_lookup: &PoolAliasLookup,
    ) -> Option<StructurePosition> {
        if max_distance_from_center.horizontal > MAX_TOTAL_STRUCTURE_RANGE {
            return None;
        }

        let max_depth = max_depth.clamp(MIN_DEPTH, MAX_DEPTH);

        let actual_start_pool_id = pool_alias_lookup.lookup(start_pool_id, &mut context.random);
        let pool = TemplatePool::discover(actual_start_pool_id)?;
        let center_rotation = Rotation::from_index(context.random.next_bounded_i32(4) as u8);
        let center_element = pool.get_random_element(&mut context.random);
        if center_element.is_empty() {
            return None;
        }

        let (local_anchor_position, adjusted_position) = start_anchor(
            &center_element,
            start_jigsaw,
            start_pool_id,
            position,
            center_rotation,
            &mut context.random,
        )?;

        let mut box_ = center_element.get_bounding_box(adjusted_position, center_rotation);

        let center_x = i32::midpoint(box_.max.x, box_.min.x);
        let center_z = i32::midpoint(box_.max.z, box_.min.z);

        let bottom_y = if project_start_to_heightmap {
            position.0.y
                + context
                    .height_sampler
                    .as_mut()
                    .map_or(0, |sampler| sampler.estimate_height(center_x, center_z))
        } else {
            adjusted_position.0.y
        };

        let ground_level_delta = center_element.get_ground_level_delta();
        let old_absolute_ground_y = box_.min.y + ground_level_delta;
        let y_offset = bottom_y - old_absolute_ground_y;
        box_.move_pos(0, y_offset, 0);
        let piece_pos = adjusted_position.add(0, y_offset, 0);

        let max_y = context.min_y + context.height as i32 - 1;
        if !start_fits_dimension_padding(context.min_y, max_y, dimension_padding, &box_) {
            return None;
        }

        let center_y = bottom_y + local_anchor_position.y;

        let global_bounding_box = global_bounds(
            Vector3::new(center_x, center_y, center_z),
            max_distance_from_center,
            context.min_y,
            max_y,
            dimension_padding,
        );

        let center_piece = Box::new(PoolElementStructurePiece {
            piece: StructurePiece::new(StructurePieceType::Jigsaw, box_, 0),
            element: center_element,
            pos: piece_pos,
            rotation: center_rotation,
            mirror: Mirror::None,
            jigsaw_blocks: Vec::new(),
            junctions: Vec::new(),
            ground_level_delta,
            liquid_settings,
            projection: pool.elements[0].projection,
        });

        let collector = collect_pieces(
            context,
            max_depth,
            center_piece,
            FreeSpace::new(global_bounding_box, box_),
            do_expansion_hack,
            pool_alias_lookup,
            liquid_settings,
        );

        Some(StructurePosition {
            start_pos: BlockPos::new(center_x, center_y, center_z),
            collector: Arc::new(std::sync::Mutex::new(collector)),
        })
    }
}

/// The start position moved so that the named start jigsaw of the center element sits on it,
/// with that jigsaw's offset; the position itself when no start jigsaw is named, and `None` when
/// the element lacks it.
fn start_anchor(
    center_element: &PoolElement,
    start_jigsaw: Option<&str>,
    start_pool_id: &str,
    position: BlockPos,
    center_rotation: Rotation,
    random: &mut RandomGenerator,
) -> Option<(Vector3<i32>, BlockPos)> {
    if let Some(target_jigsaw_id) = start_jigsaw {
        let mut found_anchor = None;
        let center_jigsaws =
            center_element.get_shuffled_jigsaw_blocks(position, center_rotation, random);
        for jigsaw in center_jigsaws {
            if jigsaw.name == target_jigsaw_id {
                found_anchor = Some(jigsaw.pos);
                break;
            }
        }

        let Some(anchor) = found_anchor else {
            tracing::error!(
                "No starting jigsaw {} found in start pool {}",
                target_jigsaw_id,
                start_pool_id
            );
            return None;
        };
        let local_anchor = anchor.0.sub(&position.0);
        Some((local_anchor, BlockPos(position.0.sub(&local_anchor))))
    } else {
        Some((Vector3::new(0, 0, 0), position))
    }
}

/// Every piece of the structure: the center piece alone at depth 0, else the center piece and
/// all children the placer attaches inside the global free space.
fn collect_pieces(
    context: &mut StructureGeneratorContext,
    max_depth: i32,
    center_piece: Box<PoolElementStructurePiece>,
    global_free: FreeSpace,
    do_expansion_hack: bool,
    pool_alias_lookup: &PoolAliasLookup,
    liquid_settings: LiquidSettings,
) -> super::StructurePiecesCollector {
    let mut collector = super::StructurePiecesCollector::new();

    if max_depth > 0 {
        let global_free = Rc::new(RefCell::new(global_free));

        let mut height_sampler = context
            .height_sampler
            .as_mut()
            .map(|s| &mut **s as &mut dyn HeightSampler);
        let mut run = Run {
            do_expansion_hack,
            height_sampler: &mut height_sampler,
            random: &mut context.random,
            pool_alias_lookup,
            liquid_settings,
        };

        Placer::place_all(
            max_depth,
            center_piece,
            global_free,
            &mut run,
            &mut collector,
        );
    } else {
        collector.add_piece(center_piece);
    }
    collector
}

/// The box children may fill: the maximum distance around the center, cut at the dimension
/// padding vertically.
fn global_bounds(
    center: Vector3<i32>,
    max_distance_from_center: &MaxDistance,
    min_y: i32,
    max_y: i32,
    dimension_padding: DimensionPadding,
) -> BlockBox {
    let Vector3 {
        x: center_x,
        y: center_y,
        z: center_z,
    } = center;
    BlockBox::new(
        center_x - max_distance_from_center.horizontal,
        (center_y - max_distance_from_center.vertical).max(min_y + dimension_padding.bottom),
        center_z - max_distance_from_center.horizontal,
        center_x + max_distance_from_center.horizontal,
        (center_y + max_distance_from_center.vertical).min(max_y - dimension_padding.top),
        center_z + max_distance_from_center.horizontal,
    )
}

/// Whether the center piece keeps the dimension padding from the world's height limits.
fn start_fits_dimension_padding(
    min_y: i32,
    max_y: i32,
    dimension_padding: DimensionPadding,
    box_: &BlockBox,
) -> bool {
    if is_start_too_close_to_world_height_limits(min_y, max_y, dimension_padding, box_) {
        tracing::debug!(
            "Center piece with bounding box {:?} does not fit dimension padding {:?}",
            box_,
            dimension_padding
        );
        return false;
    }
    true
}

const fn is_start_too_close_to_world_height_limits(
    min_y: i32,
    max_y: i32,
    dimension_padding: DimensionPadding,
    center_piece_bb: &BlockBox,
) -> bool {
    if dimension_padding.top == 0 && dimension_padding.bottom == 0 {
        return false;
    }

    let min_y_with_padding = min_y + dimension_padding.bottom;
    let max_y_with_padding = max_y - dimension_padding.top;
    center_piece_bb.min.y < min_y_with_padding || center_piece_bb.max.y > max_y_with_padding
}

const fn is_box_inside(outer: &BlockBox, inner: &BlockBox) -> bool {
    inner.min.x >= outer.min.x
        && inner.max.x <= outer.max.x
        && inner.min.y >= outer.min.y
        && inner.max.y <= outer.max.y
        && inner.min.z >= outer.min.z
        && inner.max.z <= outer.max.z
}

const fn boxes_intersect(a: &BlockBox, b: &BlockBox) -> bool {
    a.max.x >= b.min.x
        && a.min.x <= b.max.x
        && a.max.y >= b.min.y
        && a.min.y <= b.max.y
        && a.max.z >= b.min.z
        && a.min.z <= b.max.z
}

#[must_use]
pub const fn rotate_pos(pos: Vector3<i32>, rotation: Rotation) -> Vector3<i32> {
    let (x, z) = rotation.rotate_offset(pos.x, pos.z);
    Vector3::new(x, pos.y, z)
}

#[must_use]
pub fn rotated_box(origin: BlockPos, size: Vector3<i32>, rotation: Rotation) -> BlockBox {
    let corner = rotate_pos(Vector3::new(size.x - 1, size.y - 1, size.z - 1), rotation);
    BlockBox::new(
        origin.0.x.min(origin.0.x + corner.x),
        origin.0.y,
        origin.0.z.min(origin.0.z + corner.z),
        origin.0.x.max(origin.0.x + corner.x),
        origin.0.y + corner.y,
        origin.0.z.max(origin.0.z + corner.z),
    )
}

#[must_use]
pub const fn rotate_direction(
    dir: pumpkin_util::BlockDirection,
    rotation: Rotation,
) -> pumpkin_util::BlockDirection {
    use pumpkin_util::BlockDirection;
    match rotation {
        Rotation::None => dir,
        Rotation::Clockwise90 => match dir {
            BlockDirection::North => BlockDirection::East,
            BlockDirection::East => BlockDirection::South,
            BlockDirection::South => BlockDirection::West,
            BlockDirection::West => BlockDirection::North,
            _ => dir,
        },
        Rotation::Rotate180 => match dir {
            BlockDirection::North => BlockDirection::South,
            BlockDirection::South => BlockDirection::North,
            BlockDirection::West => BlockDirection::East,
            BlockDirection::East => BlockDirection::West,
            _ => dir,
        },
        Rotation::CounterClockwise90 => match dir {
            BlockDirection::North => BlockDirection::West,
            BlockDirection::West => BlockDirection::South,
            BlockDirection::South => BlockDirection::East,
            BlockDirection::East => BlockDirection::North,
            _ => dir,
        },
    }
}

fn can_attach(source: &JigsawBlock, target: &JigsawBlock) -> bool {
    if source.target != target.name {
        return false;
    }
    if source.facing.opposite() != target.facing {
        return false;
    }
    if source.joint == JigsawJointType::Aligned && source.up != target.up {
        return false;
    }
    true
}
