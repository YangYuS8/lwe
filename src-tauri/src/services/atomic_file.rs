use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

/// Replace a file without exposing a partial write or damaging its previous contents.
/// Each writer owns a private temporary file in the destination directory.
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_with(path, contents, write_and_sync, persist)
}

fn write_and_sync(file: &mut File, contents: &[u8]) -> io::Result<()> {
    file.write_all(contents)?;
    file.sync_all()
}

fn persist(temporary: NamedTempFile, path: &Path) -> io::Result<()> {
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| error.error)
}

fn atomic_write_with(
    path: &Path,
    contents: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    replace: impl FnOnce(NamedTempFile, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut temporary = NamedTempFile::new_in(parent.unwrap_or_else(|| Path::new(".")))?;
    write(temporary.as_file_mut(), contents)?;
    replace(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn partial_write_failure_preserves_previous_file_and_cleans_temporary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        fs::write(&path, "[assignments]\neDP-1 = 'video-7'\n").unwrap();
        let previous = fs::read(&path).unwrap();

        let result = atomic_write_with(
            &path,
            b"[assignments]\neDP-1 = 'video-8'\n",
            |file, contents| {
                file.write_all(&contents[..5])?;
                Err(io::Error::from(io::ErrorKind::StorageFull))
            },
            persist,
        );

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::StorageFull);
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn replacement_failure_preserves_previous_file_and_cleans_temporary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        fs::write(&path, "[assignments]\neDP-1 = 'video-7'\n").unwrap();
        let previous = fs::read(&path).unwrap();

        let result = atomic_write_with(&path, b"new state", write_and_sync, |_, _| {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn simultaneous_writers_only_publish_complete_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let first = vec![b'a'; 32 * 1024];
        let second = vec![b'b'; 32 * 1024];
        atomic_write(&path, &first).unwrap();

        std::thread::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..20 {
                    atomic_write(&path, &first).unwrap();
                }
            });
            scope.spawn(|| {
                for _ in 0..20 {
                    atomic_write(&path, &second).unwrap();
                }
            });
            for _ in 0..100 {
                let current = fs::read(&path).unwrap();
                assert!(current == first || current == second);
            }
        });
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_settings_are_private_even_when_replacing_a_public_file() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        fs::write(&path, "old settings").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write(&path, b"steam_web_api_key = 'fixture-key'").unwrap();

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
