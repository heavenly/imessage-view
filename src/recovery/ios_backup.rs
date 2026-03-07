use sha1::{Digest, Sha1};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn messages_relative_path(original_path: &str) -> Option<&str> {
    original_path
        .strip_prefix("~/Library/Messages/")
        .or_else(|| {
            original_path
                .find("/Library/Messages/")
                .map(|idx| &original_path[idx + "/Library/Messages/".len()..])
        })
        .or_else(|| original_path.strip_prefix("Library/Messages/"))
}

pub fn resolve_ios_backup_path(backup_root: &Path, original_path: &str) -> Option<PathBuf> {
    let relative = messages_relative_path(original_path)?;
    let hash_input = format!("MediaDomain-{relative}");
    let hash = format!("{:x}", Sha1::digest(hash_input.as_bytes()));
    let subdir = hash.get(0..2)?;

    Some(backup_root.join(subdir).join(hash))
}

pub fn scan_for_attachment(backup_root: &Path, original_path: &str) -> Option<PathBuf> {
    let backup_path = resolve_ios_backup_path(backup_root, original_path)?;

    if backup_path.exists() {
        Some(backup_path)
    } else {
        None
    }
}

pub fn copy_from_backup(src: &Path, dst: &Path) -> io::Result<u64> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_ios_backup_path() {
        let backup_root = Path::new("/tmp/backup");
        let original = "~/Library/Messages/Attachments/3d/03/at_0_xxx/image.jpg";

        let result = resolve_ios_backup_path(backup_root, original);
        assert!(result.is_some());

        if let Some(path) = result {
            assert!(path.to_string_lossy().contains("/tmp/backup/"));
        }
    }

    #[test]
    fn test_resolve_ios_backup_path_with_absolute_home_path() {
        let backup_root = Path::new("/tmp/backup");
        let original = "/Users/test/Library/Messages/Attachments/3d/03/at_0_xxx/image.jpg";

        let result = resolve_ios_backup_path(backup_root, original);
        assert!(
            result.is_some(),
            "expected backup path for absolute home path"
        );
    }
}
