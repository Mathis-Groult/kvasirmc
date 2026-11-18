//! The steps of `ItemEntity::try_merge_with`: snapshot both stacks, ask plugins, move the items.

use std::sync::atomic::Ordering;

use pumpkin_data::item_stack::ItemStack;

use super::{ItemEntity, MERGE_MAX_COUNT};

/// The two stacks of a merge as first seen, and which item receives.
pub(super) struct MergeSnapshot {
    pub(super) expected_low: ItemStack,
    pub(super) expected_high: ItemStack,
    pub(super) target_is_self: bool,
}

impl ItemEntity {
    /// Both stacks as they are before the merge and which item receives (vanilla: the one with
    /// more items; `other` on a tie), or `None` when the stacks cannot merge.
    pub(super) fn merge_snapshot(
        &self,
        other: &Self,
        low: &Self,
        high: &Self,
    ) -> Option<MergeSnapshot> {
        let low_stack = low
            .item_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let high_stack = high
            .item_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (self_stack, other_stack) = if self.entity.entity_id < other.entity.entity_id {
            (&*low_stack, &*high_stack)
        } else {
            (&*high_stack, &*low_stack)
        };

        if !Self::are_mergeable(self_stack, other_stack) {
            return None;
        }

        Some(MergeSnapshot {
            expected_low: low_stack.clone(),
            expected_high: high_stack.clone(),
            target_is_self: other_stack.item_count < self_stack.item_count,
        })
    }

    /// Fires the plugin merge event; false when it was cancelled or either item went away.
    pub(super) fn merge_event_allows(&self, other: &Self, target: &Self, source: &Self) -> bool {
        let mut event = crate::plugin::api::events::entity::item_merge::ItemMergeEvent {
            entity_id: target.entity.entity_id,
            target_id: source.entity.entity_id,
            cancelled: false,
        };
        let server = self.entity.world.load().server.upgrade();
        if let Some(server) = server {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        !(event.cancelled
            || self.entity.removed.load(Ordering::SeqCst)
            || other.entity.removed.load(Ordering::SeqCst))
    }

    /// Moves items into the receiving stack (vanilla `ItemEntity.merge(to, from, 64)`) if both
    /// stacks and the receiver are still as in `snapshot`; returns whether the giving stack is
    /// empty now, or `None` when nothing moved.
    pub(super) fn move_merged_items(
        &self,
        other: &Self,
        low: &Self,
        high: &Self,
        snapshot: &MergeSnapshot,
    ) -> Option<bool> {
        let low_stack = low
            .item_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let high_stack = high
            .item_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if low_stack.uid != snapshot.expected_low.uid
            || !low_stack.are_equal(&snapshot.expected_low)
            || high_stack.uid != snapshot.expected_high.uid
            || !high_stack.are_equal(&snapshot.expected_high)
            || self.entity.removed.load(Ordering::SeqCst)
            || other.entity.removed.load(Ordering::SeqCst)
        {
            return None;
        }

        let (self_stack, other_stack) = if self.entity.entity_id < other.entity.entity_id {
            (low_stack, high_stack)
        } else {
            (high_stack, low_stack)
        };
        if !Self::are_mergeable(&self_stack, &other_stack)
            || (other_stack.item_count < self_stack.item_count) != snapshot.target_is_self
        {
            return None;
        }
        let (mut stack1, mut stack2) = if snapshot.target_is_self {
            (self_stack, other_stack)
        } else {
            (other_stack, self_stack)
        };

        // Vanilla `ItemEntity.merge(to, from, 64)`.
        let max_size = stack1.get_max_stack_size().min(MERGE_MAX_COUNT);
        let moved = stack2
            .item_count
            .min(max_size.saturating_sub(stack1.item_count));
        stack1.increment(moved);
        stack2.decrement(moved);
        Some(stack2.is_empty())
    }
}
