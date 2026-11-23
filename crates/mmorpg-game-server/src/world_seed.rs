//! The secret world seed a host draws once per world (#1111, Atlas 2.1.i1111).
//!
//! Every random draw of a zone derives from its content's public seed mixed with this one,
//! so the content compiled into every client does not tell what a creature drops or which
//! night the raid comes. The seed is canonical zone state: checkpoints carry it, so a host
//! that recovers from a canonical baseline gets it back from there. A host that recovers by
//! replaying from genesis onto a fresh adapter has to build that adapter with the same seed,
//! so it keeps the seed in a file beside its recovery directory ([`for_recovery`]). The seed
//! is never logged, projected or reported.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// A fresh seed from the operating system's randomness. Never 0, which means "no world
/// seed" (the content seed alone).
///
/// # Errors
/// Fails when the operating system has no randomness to give.
pub fn fresh() -> io::Result<u64> {
    loop {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        let seed = u64::from_le_bytes(bytes);
        if seed != 0 {
            return Ok(seed);
        }
    }
}

/// Where a host recovering from `recovery` keeps its world seed: beside the recovery
/// directory, which the shared recovery host owns whole.
#[must_use]
pub fn path_for(recovery: &Path) -> PathBuf {
    let mut name = recovery
        .file_name()
        .map_or_else(|| "recovery".into(), std::ffi::OsStr::to_os_string);
    name.push(".world-seed");
    recovery.with_file_name(name)
}

/// The world seed of a host recovering from `recovery`. With no recovery bundle the world
/// starts fresh: a new seed is drawn and stored before anything is served. With a bundle the
/// world replays from genesis and needs the seed it was created with; a bundle without one
/// fails closed rather than replaying a different world.
///
/// # Errors
/// Fails when the seed file cannot be read or written, or a bundle has no seed.
pub fn for_recovery(recovery: &Path) -> io::Result<u64> {
    let path = path_for(recovery);
    match fs::metadata(recovery) {
        Ok(_) => read(&path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "the recovery bundle {} has no readable world seed at {}: {error}",
                    recovery.display(),
                    path.display()
                ),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let seed = fresh()?;
            write(&path, seed)?;
            Ok(seed)
        }
        Err(error) => Err(error),
    }
}

fn read(path: &Path) -> io::Result<u64> {
    let bytes = fs::read(path)?;
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "a world seed is 8 bytes"))?;
    Ok(u64::from_be_bytes(bytes))
}

/// Writes the seed owner-readable only, through a temporary file renamed into place.
fn write(path: &Path, seed: u64) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary)?;
    file.write_all(&seed.to_be_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_world_stores_its_seed_and_a_recovered_one_reads_it_back() {
        let directory = tempfile::tempdir().unwrap();
        let recovery = directory.path().join("zone-recovery");
        let seed = for_recovery(&recovery).unwrap();
        assert_ne!(seed, 0);
        assert_eq!(
            path_for(&recovery),
            directory.path().join("zone-recovery.world-seed")
        );
        // While no bundle exists the world is still fresh and the stored seed is replaced.
        let fresh = for_recovery(&recovery).unwrap();
        fs::create_dir(&recovery).unwrap();
        assert_eq!(for_recovery(&recovery).unwrap(), fresh);
        assert_eq!(for_recovery(&recovery).unwrap(), fresh);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path_for(&recovery))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "the seed file is owner-only");
        }
    }

    #[test]
    fn a_bundle_without_its_seed_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let recovery = directory.path().join("zone-recovery");
        fs::create_dir(&recovery).unwrap();
        assert!(for_recovery(&recovery).is_err());
        fs::write(path_for(&recovery), [1, 2, 3]).unwrap();
        assert!(for_recovery(&recovery).is_err());
    }
}
