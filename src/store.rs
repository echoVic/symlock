use anyhow::{Context, Result};
use fs4::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::LockFile;

/// The `.symlock` directory holding lock state, discovered by walking up from CWD.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Create `.symlock/` under `root` and seed an empty lock file.
    pub fn init(root: &Path) -> Result<PathBuf> {
        let dir = root.join(".symlock");
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let locks = dir.join("locks.json");
        if !locks.exists() {
            let empty = serde_json::to_string_pretty(&LockFile::default())?;
            fs::write(&locks, empty)?;
        }
        Ok(dir)
    }

    /// Locate the `.symlock` directory.
    ///
    /// `$SYMLOCK_DIR` takes precedence when set — this is how agents in separate
    /// git worktrees (which are independent directory trees) share one lock file:
    /// point them all at the same directory. Otherwise we walk up from `start`.
    pub fn discover(start: &Path) -> Result<Store> {
        if let Ok(dir) = std::env::var("SYMLOCK_DIR") {
            let dir = PathBuf::from(dir);
            if dir.is_dir() {
                return Ok(Store { dir });
            }
            anyhow::bail!("$SYMLOCK_DIR={} is not a directory", dir.display());
        }
        let mut cur = Some(start);
        while let Some(dir) = cur {
            let candidate = dir.join(".symlock");
            if candidate.is_dir() {
                return Ok(Store { dir: candidate });
            }
            cur = dir.parent();
        }
        anyhow::bail!("no .symlock directory found (run `symlock init` first)")
    }

    fn locks_path(&self) -> PathBuf {
        self.dir.join("locks.json")
    }

    /// Run `f` with exclusive access to the lock file: acquire an OS advisory lock,
    /// read current state, let the caller mutate it, then write it back atomically.
    /// This is what makes concurrent `claim` from parallel agents race-free.
    pub fn with_exclusive<T>(&self, f: impl FnOnce(&mut LockFile) -> Result<T>) -> Result<T> {
        let path = self.locks_path();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            // We truncate manually (set_len) *after* reading current state, so we
            // must not truncate on open — that would wipe the file before we read it.
            .truncate(false)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;

        file.lock_exclusive().context("acquiring file lock")?;
        let result = (|| {
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            let mut lockfile: LockFile = if buf.trim().is_empty() {
                LockFile::default()
            } else {
                serde_json::from_str(&buf).context("parsing locks.json")?
            };

            let out = f(&mut lockfile)?;

            let serialized = serde_json::to_string_pretty(&lockfile)?;
            file.seek(SeekFrom::Start(0))?;
            file.set_len(0)?;
            file.write_all(serialized.as_bytes())?;
            file.flush()?;
            Ok(out)
        })();
        // Always release, even on error.
        let _ = FileExt::unlock(&file);
        result
    }

    /// Read-only snapshot of the current lock state.
    pub fn read(&self) -> Result<LockFile> {
        let path = self.locks_path();
        let mut file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        file.lock_shared().context("acquiring shared lock")?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        let _ = FileExt::unlock(&file);
        if buf.trim().is_empty() {
            return Ok(LockFile::default());
        }
        serde_json::from_str(&buf).context("parsing locks.json")
    }
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
