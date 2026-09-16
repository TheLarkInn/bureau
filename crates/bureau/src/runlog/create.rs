//! Pipeline construction and durable events-only construction.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::{EVENTS_FILE, RunLog, read_events, run_dir};
use crate::process::Secret;

#[cfg(test)]
mod tests;

fn parent_path(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    }
}

fn new_parents(dir: &Path) -> Vec<PathBuf> {
    dir.ancestors()
        .take_while(|path| !path.is_dir())
        .filter_map(Path::parent)
        .map(parent_path)
        .collect()
}

fn new_file(dir: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(dir.join(EVENTS_FILE))
}

fn opened(file: File, dir: PathBuf, secrets: &[Secret], next_seq: u64) -> RunLog {
    RunLog {
        writer: BufWriter::new(file),
        secrets: secrets.to_vec(),
        next_seq,
        dir,
    }
}

fn sync_created(
    file: &File,
    dir: &Path,
    parents: &[PathBuf],
    sync: &mut impl FnMut(&File) -> io::Result<()>,
) -> io::Result<()> {
    sync(file)?;
    sync(&File::open(dir)?)?;
    for parent in parents {
        sync(&File::open(parent)?)?;
    }
    Ok(())
}

fn create_events_with(
    runs_dir: &Path,
    run_id: &str,
    secrets: &[Secret],
    mut sync: impl FnMut(&File) -> io::Result<()>,
) -> io::Result<RunLog> {
    let dir = run_dir(runs_dir, run_id);
    let parents = new_parents(&dir);
    std::fs::create_dir_all(&dir)?;
    let file = new_file(&dir)?;
    sync_created(&file, &dir, &parents, &mut sync)?;
    Ok(opened(file, dir, secrets, 0))
}

fn finish_line(file: &mut File) -> io::Result<()> {
    if file.metadata()?.len() == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0];
    file.read_exact(&mut last)?;
    if last != [b'\n'] {
        file.write_all(b"\n")?;
    }
    Ok(())
}

impl RunLog {
    /// Creates the pipeline run directory, worktree/artifact directories,
    /// and log. A run id is used exactly once.
    ///
    /// # Errors
    /// Propagates filesystem failures, including an existing log.
    pub fn create(runs_dir: &Path, run_id: &str, secrets: &[Secret]) -> io::Result<Self> {
        let dir = run_dir(runs_dir, run_id);
        std::fs::create_dir_all(dir.join("artifacts"))?;
        std::fs::create_dir_all(dir.join("wt"))?;
        Ok(opened(new_file(&dir)?, dir, secrets, 0))
    }

    /// Creates only a run directory and events log. The new file and
    /// containing directory entries are synced before returning.
    ///
    /// # Errors
    /// Propagates every creation and sync failure; refuses an existing log.
    pub fn create_events(runs_dir: &Path, run_id: &str, secrets: &[Secret]) -> io::Result<Self> {
        create_events_with(runs_dir, run_id, secrets, File::sync_all)
    }

    /// Opens an existing log after repairing and syncing any torn final line.
    /// A complete record missing its final newline keeps a separate next append.
    ///
    /// # Errors
    /// Propagates filesystem and sync failures; rejects corrupt earlier events.
    pub fn resume(dir: &Path, secrets: &[Secret]) -> io::Result<Self> {
        let events = read_events(dir)?;
        let next_seq = events.last().map_or(0, |event| event.seq + 1);
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(dir.join(EVENTS_FILE))?;
        finish_line(&mut file)?;
        file.sync_all()?;
        Ok(opened(file, dir.to_path_buf(), secrets, next_seq))
    }
}
