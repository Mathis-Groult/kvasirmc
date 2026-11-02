//! The job as it is saved between runs: what to generate and how far it got.

use serde::{Deserialize, Serialize};

use crate::{
    shape::{ChunkPos, Shape, ShapeError},
    spiral::{Cursor, Spiral, ring_len},
};

/// Version of the file layout. A file with another number is refused, not guessed at.
pub const SCHEMA: u32 = 1;

/// A pre-generation job, saved after every progress step so that a stop or crash loses nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub schema: u32,
    /// The dimension, such as `minecraft:overworld`.
    pub world: String,
    pub centre: ChunkPos,
    pub shape: Shape,
    /// Where to continue (see [`crate::InFlight::resume_cursor`]).
    pub cursor: Cursor,
    /// Chunks finished so far.
    pub done: u64,
    /// Stopped by the owner; a restart leaves it stopped.
    pub paused: bool,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JobError {
    #[error("the job file is not valid: {0}")]
    Parse(String),
    #[error("the job could not be written: {0}")]
    Render(String),
    #[error("the job file has layout {found}, this engine reads layout {SCHEMA}")]
    Schema { found: u32 },
    #[error(transparent)]
    Shape(#[from] ShapeError),
    #[error("the job file claims {done} chunks done of {total}")]
    DoneAboveTotal { done: u64, total: u64 },
    #[error("the job file's cursor {cursor:?} is outside a spiral of radius {radius}")]
    CursorOutside { cursor: Cursor, radius: u32 },
}

impl Job {
    /// A new job at the start of its spiral.
    ///
    /// # Errors
    /// The shape does not fit around the centre.
    pub fn new(
        world: impl Into<String>,
        centre: ChunkPos,
        shape: Shape,
    ) -> Result<Self, ShapeError> {
        shape.check_around(centre)?;
        Ok(Self {
            schema: SCHEMA,
            world: world.into(),
            centre,
            shape,
            cursor: Cursor::default(),
            done: 0,
            paused: false,
        })
    }

    #[must_use]
    pub fn total(&self) -> u64 {
        self.shape.chunk_count()
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.done >= self.total()
    }

    /// The chunks still to do, starting at the saved cursor.
    #[must_use]
    pub const fn spiral(&self) -> Spiral {
        Spiral::resume(self.shape, self.centre, self.cursor)
    }

    /// Reads a saved job. The file may have been edited or damaged, so every field is checked.
    ///
    /// # Errors
    /// The text is not a job of this layout, or its numbers contradict each other.
    pub fn parse(text: &str) -> Result<Self, JobError> {
        let job: Self = serde_json::from_str(text).map_err(|e| JobError::Parse(e.to_string()))?;
        if job.schema != SCHEMA {
            return Err(JobError::Schema { found: job.schema });
        }
        job.shape.check_around(job.centre)?;
        let total = job.total();
        if job.done > total {
            return Err(JobError::DoneAboveTotal {
                done: job.done,
                total,
            });
        }
        let radius = job.shape.radius();
        let Cursor { ring, index } = job.cursor;
        if ring > radius + 1 || index >= ring_len(ring) {
            return Err(JobError::CursorOutside {
                cursor: job.cursor,
                radius,
            });
        }
        Ok(job)
    }

    /// The text to save.
    ///
    /// # Errors
    /// Serialising failed (it cannot for a job made by this crate, which holds only numbers and strings).
    pub fn render(&self) -> Result<String, JobError> {
        serde_json::to_string_pretty(self).map_err(|e| JobError::Render(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Job {
        Job::new(
            "minecraft:overworld",
            ChunkPos { x: 3, z: -2 },
            Shape::Circle { radius: 20 },
        )
        .unwrap()
    }

    #[test]
    fn a_new_job_starts_at_the_centre() {
        let job = job();
        assert_eq!(job.done, 0);
        assert_eq!(job.total(), 1257);
        assert_eq!(job.spiral().next(), Some(ChunkPos { x: 3, z: -2 }));
        assert!(!job.is_finished());
    }

    #[test]
    fn what_is_rendered_is_read_back_unchanged() {
        let mut saved = job();
        saved.cursor = Cursor { ring: 7, index: 11 };
        saved.done = 130;
        saved.paused = true;
        assert_eq!(Job::parse(&saved.render().unwrap()).unwrap(), saved);
    }

    #[test]
    fn a_finished_job_knows_it() {
        let mut finished = job();
        finished.done = finished.total();
        finished.cursor = Cursor { ring: 21, index: 0 };
        let reread = Job::parse(&finished.render().unwrap()).unwrap();
        assert!(reread.is_finished());
        assert_eq!(reread.spiral().count(), 0);
    }

    #[test]
    fn refuses_files_that_contradict_themselves() {
        let mut bad = job();
        bad.done = bad.total() + 1;
        assert!(matches!(
            Job::parse(&bad.render().unwrap()),
            Err(JobError::DoneAboveTotal { .. })
        ));
        let mut bad = job();
        bad.cursor = Cursor { ring: 3, index: 24 }; // ring 3 has 24 cells: index 24 is outside
        assert!(matches!(
            Job::parse(&bad.render().unwrap()),
            Err(JobError::CursorOutside { .. })
        ));
        let mut bad = job();
        bad.cursor = Cursor {
            ring: 500,
            index: 0,
        };
        assert!(matches!(
            Job::parse(&bad.render().unwrap()),
            Err(JobError::CursorOutside { .. })
        ));
        let mut bad = job();
        bad.schema = 2;
        assert!(matches!(
            Job::parse(&bad.render().unwrap()),
            Err(JobError::Schema { found: 2 })
        ));
    }

    #[test]
    fn refuses_a_shape_that_leaves_the_world() {
        let near_edge = Job::new(
            "minecraft:overworld",
            ChunkPos {
                x: i32::MAX - 1,
                z: 0,
            },
            Shape::Square { radius: 5 },
        );
        assert!(near_edge.is_err());
        let text = r#"{"schema":1,"world":"w","centre":{"x":2147483647,"z":0},
            "shape":{"kind":"square","radius":5},"cursor":{"ring":0,"index":0},"done":0,"paused":false}"#;
        assert!(matches!(Job::parse(text), Err(JobError::Shape(_))));
    }

    #[test]
    fn refuses_text_that_is_not_a_job() {
        assert!(matches!(Job::parse("{"), Err(JobError::Parse(_))));
        assert!(matches!(Job::parse("{}"), Err(JobError::Parse(_))));
    }
}
