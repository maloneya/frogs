//! Shared admission for local control sockets. Never displace a live peer or file.
use std::{fs::{File, Permissions}, io, os::unix::{fs::{FileTypeExt, PermissionsExt}, net::{UnixListener, UnixStream}}, path::Path};

pub(crate) fn bind(path: &Path) -> io::Result<UnixListener> {
    let path = std::path::absolute(path)?;
    // Serialize cooperating binders through stale-socket removal and bind.
    let directory = File::open(path.parent().ok_or_else(|| io::Error::other("Socket path needs a parent directory"))?)?;
    directory.lock()?;
    match std::fs::symlink_metadata(&path) {
        Ok(meta) => {
            if !meta.file_type().is_socket() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "Path is not a socket"));
            }
            match UnixStream::connect(&path) {
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => std::fs::remove_file(&path)?,
                _ => return Err(io::Error::new(io::ErrorKind::AddrInUse, "Socket is already in use")),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_preserves_files_symlinks_and_live_peers_but_reclaims_stale_sockets() {
        let directory = std::env::temp_dir().join(format!("arpg-local-socket-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("control.sock");
        let file = directory.join("keep");
        std::fs::write(&file, b"preserve me").unwrap();
        assert!(bind(&file).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"preserve me");
        std::os::unix::fs::symlink(&file, &path).unwrap();
        assert!(bind(&path).is_err());
        assert_eq!(std::fs::read_link(&path).unwrap(), file);
        std::fs::remove_file(&path).unwrap();

        let first = bind(&path).unwrap();
        assert!(bind(&path).is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // The refused attempt must leave the original listener reachable.
        let client = UnixStream::connect(&path).unwrap();
        first.accept().unwrap();
        drop(client);
        drop(first);
        let replacement = bind(&path).unwrap();
        drop(replacement);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
