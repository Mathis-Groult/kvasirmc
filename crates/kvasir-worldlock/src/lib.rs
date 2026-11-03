//! One engine per world.
//!
//! Two engines writing the same region files corrupt them silently (measured: Pumpkin opens a
//! second engine on the same `world/` without a word). The lock is an exclusive `flock` on a file
//! in the world folder. The kernel drops it when the process ends, so a crash or `kill -9` never
//! leaves a stale lock behind, and a lock file left on disk means nothing by itself.

// Tests state expectations with `unwrap`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// The file the lock is taken on, inside the world folder.
pub const LOCK_FILE: &str = "kvasir.lock";

/// Why the lock could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process holds it.
    #[error(
        "the world {world} is already in use by another server{holder}; stop it first (two servers on one world corrupt it)"
    )]
    Busy {
        world: PathBuf,
        /// ` (process 1234)` when the holder wrote its id, empty otherwise.
        holder: String,
    },
    #[error("cannot lock the world {world}: {source}")]
    Io {
        world: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The lock. Dropping it releases it.
#[derive(Debug)]
pub struct WorldLock {
    _file: File,
}

impl WorldLock {
    /// Takes the lock on `world`, creating the folder if it does not exist yet.
    ///
    /// # Errors
    /// [`LockError::Busy`] when another process holds the lock; [`LockError::Io`] when the folder
    /// or the lock file cannot be used.
    pub fn acquire(world: &Path) -> Result<Self, LockError> {
        let io_error = |source| LockError::Io {
            world: world.to_path_buf(),
            source,
        };
        fs::create_dir_all(world).map_err(io_error)?;
        let path = world.join(LOCK_FILE);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_error)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let text = fs::read_to_string(&path).unwrap_or_default();
                let holder = text
                    .trim()
                    .parse::<u32>()
                    .map(|pid| format!(" (process {pid})"))
                    .unwrap_or_default();
                return Err(LockError::Busy {
                    world: world.to_path_buf(),
                    holder,
                });
            }
            Err(TryLockError::Error(source)) => return Err(io_error(source)),
        }
        // Say who holds it, for the next one to fail with a useful message.
        file.set_len(0).map_err(io_error)?;
        write!(file, "{}", std::process::id()).map_err(io_error)?;
        file.flush().map_err(io_error)?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn the_first_engine_gets_the_lock_and_a_second_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let first = WorldLock::acquire(dir.path()).unwrap();
        let second = WorldLock::acquire(dir.path()).unwrap_err();
        assert!(matches!(&second, LockError::Busy { .. }), "{second}");
        drop(first);
    }

    #[test]
    fn the_refusal_names_the_world_and_the_holder() {
        let dir = tempfile::tempdir().unwrap();
        let _first = WorldLock::acquire(dir.path()).unwrap();
        let text = WorldLock::acquire(dir.path()).unwrap_err().to_string();
        assert!(text.contains(&dir.path().display().to_string()), "{text}");
        assert!(
            text.contains(&format!("(process {})", std::process::id())),
            "{text}"
        );
        assert!(text.contains("stop it first"), "{text}");
    }

    #[test]
    fn dropping_the_lock_lets_the_next_engine_in() {
        let dir = tempfile::tempdir().unwrap();
        drop(WorldLock::acquire(dir.path()).unwrap());
        assert!(WorldLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn a_missing_world_folder_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("new/world");
        let _lock = WorldLock::acquire(&world).unwrap();
        assert!(world.join(LOCK_FILE).is_file());
    }

    #[test]
    fn a_lock_file_left_behind_by_a_dead_engine_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOCK_FILE), "424242").unwrap();
        let lock = WorldLock::acquire(dir.path()).unwrap();
        let written = fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap();
        assert_eq!(
            written,
            std::process::id().to_string(),
            "the stale id is replaced"
        );
        drop(lock);
    }

    #[test]
    fn a_shorter_id_leaves_no_digits_of_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOCK_FILE), "99999999999999999999").unwrap();
        let _lock = WorldLock::acquire(dir.path()).unwrap();
        let written = fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap();
        assert_eq!(written, std::process::id().to_string());
    }

    #[test]
    fn a_world_that_cannot_be_a_folder_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain-file");
        fs::write(&file, "x").unwrap();
        assert!(matches!(
            WorldLock::acquire(&file),
            Err(LockError::Io { .. })
        ));
    }

    /// The reference is util-linux's `flock(1)`, a tool that is not ours: it takes the same kind
    /// of lock from another process, holds it for two seconds and exits.
    #[test]
    fn a_lock_held_by_another_program_refuses_us_and_its_exit_frees_it() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(LOCK_FILE);
        let mut holder = Command::new("flock")
            .arg("-n")
            .arg(&lock_path)
            .args(["sleep", "2"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("flock(1) from util-linux");
        // Do not probe while it starts: a probe that wins would make `flock -n` give up.
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(matches!(
            WorldLock::acquire(dir.path()),
            Err(LockError::Busy { .. })
        ));
        holder.wait().unwrap();
        assert!(
            WorldLock::acquire(dir.path()).is_ok(),
            "released when the holder exited"
        );
    }
}
