//! Folders of the emulator's own in the computer's temporary folder: made
//! fresh (never one that is there already, which another user of the
//! computer could have put there), with a random name, open to this user
//! alone where the system has such rights (0700).

use std::io;
use std::path::{Path, PathBuf};

/// A new, empty folder of this user's alone in the temporary folder, its
/// name starting with `prefix`.
pub fn dir(prefix: &str) -> io::Result<PathBuf> {
    dir_in(&std::env::temp_dir(), prefix)
}

/// [`dir`] in `parent`.
pub fn dir_in(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    for _ in 0..16 {
        let mut bytes = [0u8; 8];
        if !crate::host::random(&mut bytes) {
            return Err(io::Error::other("no random numbers for a folder's name"));
        }
        let name: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let path = parent.join(format!("{prefix}-{}-{name}", std::process::id()));
        #[cfg(unix)]
        let builder = {
            let mut builder = std::fs::DirBuilder::new();
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = std::fs::DirBuilder::new();
        // Made here or not at all: one that is there is someone else's.
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no fresh folder could be made"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each folder is new and empty, and never one that was there.
    #[test]
    fn folders_are_fresh_and_their_own() {
        let a = dir("paperpaw-private-test").unwrap();
        let b = dir("paperpaw-private-test").unwrap();
        assert_ne!(a, b);
        assert!(a.is_dir() && std::fs::read_dir(&a).unwrap().next().is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&a).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
