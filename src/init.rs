use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "skill-lock.toml";

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
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| match err.kind() {
            io::ErrorKind::AlreadyExists => InitError::AlreadyExists(path.clone()),
            _ => InitError::Io(path.clone(), err),
        })?;
    file.write_all(TEMPLATE.as_bytes())
        .map_err(|err| InitError::Io(path.clone(), err))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
