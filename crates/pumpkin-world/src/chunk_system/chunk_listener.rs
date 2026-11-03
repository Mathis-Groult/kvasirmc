use super::ChunkPos;
use crate::level::SyncChunk;
use crossbeam::channel::{Receiver, Sender};
use std::sync::Arc;
use std::sync::{Mutex, Weak};
use tokio::sync::oneshot;

#[expect(clippy::type_complexity)]
pub struct ChunkListener {
    single: Mutex<Vec<(ChunkPos, oneshot::Sender<SyncChunk>)>>,
    global: Mutex<Vec<Sender<(ChunkPos, Weak<crate::chunk::ChunkData>)>>>,
}

impl Default for ChunkListener {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkListener {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            single: Mutex::new(Vec::new()),
            global: Mutex::new(Vec::new()),
        }
    }

    pub fn add_single_chunk_listener(&self, pos: ChunkPos) -> oneshot::Receiver<SyncChunk> {
        let (tx, rx) = oneshot::channel();
        self.single
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((pos, tx));
        rx
    }

    /// Drops the single-chunk listeners whose receiver has gone away, so a caller that no longer
    /// waits does not leave an entry behind until its chunk is next published.
    pub fn forget_abandoned(&self) {
        self.single
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(_, sender)| !sender.is_closed());
    }

    pub fn add_global_chunk_listener(&self) -> Receiver<(ChunkPos, Weak<crate::chunk::ChunkData>)> {
        let (tx, rx) = crossbeam::channel::unbounded();
        self.global
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(tx);
        rx
    }

    pub fn process_new_chunk(&self, pos: ChunkPos, chunk: &SyncChunk) {
        {
            let mut single = self
                .single
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut i = 0;
            let mut len = single.len();
            while i < len {
                if single[i].0 == pos {
                    let (_, send) = single.remove(i);
                    let _ = send.send(chunk.clone());
                    // log::debug!("single listener {i} send {pos:?}");
                    len -= 1;
                    continue;
                }
                i += 1;
            }
        }
        {
            let weak = Arc::downgrade(chunk);
            let mut global = self
                .global
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut i = 0;
            let mut len = global.len();
            while i < len {
                if matches!(global[i].send((pos, weak.clone())), Ok(())) {
                    // log::debug!("global listener {i} send {pos:?}");
                } else {
                    // log::debug!("one global listener dropped");
                    global.remove(i);
                    len -= 1;
                    continue;
                }
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_util::math::vector2::Vector2;

    use super::*;
    use crate::chunk::ChunkData;

    fn chunk(x: i32, z: i32) -> SyncChunk {
        ChunkData::empty_sync(x, z)
    }

    #[tokio::test]
    async fn a_waiting_listener_receives_its_chunk_when_it_is_published() {
        let listener = ChunkListener::new();
        let wanted = listener.add_single_chunk_listener(Vector2::new(3, 4));
        let other = listener.add_single_chunk_listener(Vector2::new(9, 9));
        listener.process_new_chunk(Vector2::new(3, 4), &chunk(3, 4));
        assert!(wanted.await.is_ok());
        drop(other);
    }

    #[test]
    fn forgetting_abandoned_listeners_drops_closed_ones_and_keeps_waiting_ones() {
        let listener = ChunkListener::new();
        let abandoned = listener.add_single_chunk_listener(Vector2::new(1, 1));
        let _waiting = listener.add_single_chunk_listener(Vector2::new(2, 2));
        drop(abandoned);
        assert_eq!(listener.single.lock().unwrap().len(), 2);
        listener.forget_abandoned();
        let left = listener.single.lock().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].0, Vector2::new(2, 2));
    }
}
