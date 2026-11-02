//! The job as a file next to the world: written whole, then moved into place, so a crash leaves the
//! old file or the new one and never half of either.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use kvasir_pregen::{Job, JobError};

use crate::JobStore;

/// A job file at a fixed path.
///
/// Saves are not flushed to the disk platter: a crash of the process loses nothing, a crash of the
/// machine may lose the last second or leave a file that fails to parse, and the next start then
/// reports it instead of resuming.
#[derive(Debug, Clone)]
pub struct FileStore {
    path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("{path} is not a usable job: {source}")]
    Invalid { path: PathBuf, source: JobError },
}

impl FileStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn temporary(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".tmp");
        self.path.with_file_name(name)
    }

    /// The saved job, if there is one.
    ///
    /// # Errors
    /// The file exists and cannot be read, or does not hold a valid job.
    pub fn load(&self) -> Result<Option<Job>, LoadError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(LoadError::Read {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        Job::parse(&text)
            .map(Some)
            .map_err(|source| LoadError::Invalid {
                path: self.path.clone(),
                source,
            })
    }
}

impl JobStore for FileStore {
    fn save(&self, job: &Job) -> io::Result<()> {
        let text = job.render().map_err(io::Error::other)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.temporary();
        fs::write(&temporary, text)?;
        fs::rename(&temporary, &self.path)
    }

    fn clear(&self) -> io::Result<()> {
        for path in [&self.path, &self.temporary()] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use kvasir_pregen::{ChunkPos, Cursor, Shape};

    use super::*;

    fn job() -> Job {
        let mut job = Job::new(
            "minecraft:the_nether",
            ChunkPos { x: -7, z: 12 },
            Shape::Circle { radius: 9 },
        )
        .unwrap();
        job.cursor = Cursor { ring: 3, index: 5 };
        job.done = 20;
        job
    }

    #[test]
    fn saves_in_a_folder_that_does_not_exist_yet_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().join("kvasir/pregen.json"));
        assert_eq!(store.load().unwrap(), None);
        store.save(&job()).unwrap();
        assert_eq!(store.load().unwrap(), Some(job()));
    }

    #[test]
    fn a_second_save_replaces_the_first_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().join("pregen.json"));
        store.save(&job()).unwrap();
        let mut later = job();
        later.done = 99;
        later.cursor = Cursor { ring: 4, index: 0 };
        store.save(&later).unwrap();
        assert_eq!(store.load().unwrap(), Some(later));
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["pregen.json"]);
    }

    #[test]
    fn clearing_removes_the_file_and_is_harmless_twice() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().join("pregen.json"));
        store.save(&job()).unwrap();
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.clear().unwrap();
    }

    #[test]
    fn clearing_also_removes_a_temporary_file_a_crash_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().join("pregen.json"));
        fs::write(dir.path().join("pregen.json.tmp"), "{").unwrap();
        store.clear().unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_file_that_is_not_a_job_is_reported_not_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pregen.json");
        fs::write(&path, "{\"schema\":1").unwrap();
        let error = FileStore::new(&path).load().unwrap_err();
        assert!(matches!(error, LoadError::Invalid { .. }), "{error}");
        assert!(error.to_string().contains("pregen.json"));
        assert!(
            path.exists(),
            "the damaged file is left for the owner to look at"
        );
    }

    #[test]
    fn an_unreadable_path_is_a_read_error() {
        let dir = tempfile::tempdir().unwrap();
        // a directory where the file should be
        let path = dir.path().join("pregen.json");
        fs::create_dir(&path).unwrap();
        assert!(matches!(
            FileStore::new(&path).load().unwrap_err(),
            LoadError::Read { .. }
        ));
    }
}
