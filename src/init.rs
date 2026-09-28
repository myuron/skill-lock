use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub use crate::manifest::MANIFEST_FILE;

const TEMPLATE: &str = r#"version = 1

[settings]
targets = ["claude"]

# [skills.pdf]
# github = "anthropics/skills"
# path = "skills/pdf"
# tag = "v1.2.0"
"#;

#[derive(Debug)]
pub enum InitError {
    AlreadyExists(PathBuf),
    Io(PathBuf, io::Error),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::AlreadyExists(path) => write!(f, "{} already exists", path.display()),
            InitError::Io(path, err) => write!(f, "failed to write {}: {err}", path.display()),
        }
    }
}

impl std::error::Error for InitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InitError::AlreadyExists(_) => None,
            InitError::Io(_, err) => Some(err),
        }
    }
}

/// `dir` に `skill-lock.toml` の雛形を作成する。既に存在する場合はエラー。
pub fn run(dir: &Path) -> Result<PathBuf, InitError> {
    let path = dir.join(MANIFEST_FILE);
    create_new_with(&path, |file| file.write_all(TEMPLATE.as_bytes()))?;
    Ok(path)
}

/// `path` を新規作成して `write` で書き込む。書き込みに失敗した場合は作成したファイルを削除する。
fn create_new_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), InitError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| match err.kind() {
            io::ErrorKind::AlreadyExists => InitError::AlreadyExists(path.to_path_buf()),
            _ => InitError::Io(path.to_path_buf(), err),
        })?;
    if let Err(err) = write(&mut file) {
        drop(file);
        // 削除の失敗は無視し、元の書き込みエラーを返す
        let _ = fs::remove_file(path);
        return Err(InitError::Io(path.to_path_buf(), err));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_template() {
        let dir = tempfile::tempdir().unwrap();
        let path = run(dir.path()).unwrap();
        assert_eq!(path, dir.path().join(MANIFEST_FILE));
        assert_eq!(fs::read_to_string(&path).unwrap(), TEMPLATE);
    }

    #[test]
    fn fails_if_manifest_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MANIFEST_FILE);
        fs::write(&path, "existing").unwrap();
        assert!(matches!(run(dir.path()), Err(InitError::AlreadyExists(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "existing");
    }

    #[test]
    fn removes_file_if_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MANIFEST_FILE);
        let result = create_new_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("disk full"))
        });
        assert!(matches!(result, Err(InitError::Io(_, _))));
        assert!(!path.exists());
        // ファイルが残っていないので再実行できる
        run(dir.path()).unwrap();
    }
}
