//! The database file's existence and permissions.
//!
//! It holds password hashes, so it is created owner-only (0600). SQLite
//! gives the `-wal` and `-shm` files it creates beside it the database
//! file's own mode.

use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;

use crate::{OpenMode, StoreError};

/// Permission bits for anyone but the owner.
const OTHERS: u32 = 0o077;

/// Creates `path` with mode 0600 when it is missing and `open` allows;
/// otherwise checks that it exists and only its owner may read or write it.
pub(crate) fn prepare(path: &Path, open: OpenMode) -> Result<(), StoreError> {
    let file_error = |source| StoreError::File {
        path: path.to_owned(),
        source,
    };
    if open == OpenMode::CreateIfMissing {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(file_error(error)),
        }
    }
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(StoreError::NoDatabase {
                path: path.to_owned(),
            });
        }
        Err(error) => return Err(file_error(error)),
    };
    let mode = metadata.permissions().mode();
    if mode & OTHERS != 0 {
        return Err(StoreError::InsecurePermissions {
            path: path.to_owned(),
            mode: mode & 0o7777,
        });
    }
    Ok(())
}
