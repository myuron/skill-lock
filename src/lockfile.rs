use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::manifest::Requested;

pub const LOCK_FILE: &str = "skill-lock.lock";

const SUPPORTED_VERSION: u32 = 1;
const HEADER: &str = "# このファイルは skill-lock が自動生成します。手で編集しないでください。\n";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lockfile {
    pub skills: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub github: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub requested: Requested,
    pub commit: String,
    pub hash: String,
    pub targets: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLockfile {
    version: u32,
    #[serde(default)]
    skill: Vec<Entry>,
}

#[derive(Debug)]
pub enum LockfileError {
    Io(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
    UnsupportedVersion(PathBuf, u32),
    Invalid(PathBuf, String),
}

impl fmt::Display for LockfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockfileError::Io(path, err) => write!(f, "failed to access {}: {err}", path.display()),
            LockfileError::Parse(path, err) => {
                write!(f, "failed to parse {}: {err}", path.display())
            }
            LockfileError::UnsupportedVersion(path, version) => write!(
                f,
                "{} has unsupported version {version} (supported: {SUPPORTED_VERSION})",
                path.display()
            ),
            LockfileError::Invalid(path, msg) => write!(f, "invalid {}: {msg}", path.display()),
        }
    }
}

impl std::error::Error for LockfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockfileError::Io(_, err) => Some(err),
            LockfileError::Parse(_, err) => Some(err),
            LockfileError::UnsupportedVersion(_, _) | LockfileError::Invalid(_, _) => None,
        }
    }
}

/// `path` の lock ファイルを読み込む。存在しない場合は `None`。
pub fn read(path: &Path) -> Result<Option<Lockfile>, LockfileError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(LockfileError::Io(path.to_path_buf(), err)),
    };
    let raw: RawLockfile =
        toml::from_str(&text).map_err(|err| LockfileError::Parse(path.to_path_buf(), err))?;
    if raw.version != SUPPORTED_VERSION {
        return Err(LockfileError::UnsupportedVersion(
            path.to_path_buf(),
            raw.version,
        ));
    }
    for entry in &raw.skill {
        if !is_commit_sha(&entry.commit) {
            return Err(LockfileError::Invalid(
                path.to_path_buf(),
                format!(
                    "skill {}: commit must be a 40 digit hex SHA: {:?}",
                    entry.name, entry.commit
                ),
            ));
        }
    }
    Ok(Some(Lockfile { skills: raw.skill }))
}

/// lock ファイルの内容を文字列にする。`[[skill]]` は name の昇順で出力する。
pub fn render(lockfile: &Lockfile) -> String {
    let mut skills: Vec<&Entry> = lockfile.skills.iter().collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = String::new();
    out.push_str(HEADER);
    out.push_str(&format!("version = {SUPPORTED_VERSION}\n"));
    for entry in skills {
        out.push_str("\n[[skill]]\n");
        out.push_str(&format!("name = {}\n", quote(&entry.name)));
        out.push_str(&format!("github = {}\n", quote(&entry.github)));
        if let Some(path) = &entry.path {
            out.push_str(&format!("path = {}\n", quote(path)));
        }
        let requested = match &entry.requested {
            Requested::Default => "{}".to_string(),
            Requested::Tag(tag) => format!("{{ tag = {} }}", quote(tag)),
            Requested::Branch(branch) => format!("{{ branch = {} }}", quote(branch)),
            Requested::Rev(rev) => format!("{{ rev = {} }}", quote(rev)),
        };
        out.push_str(&format!("requested = {requested}\n"));
        out.push_str(&format!("commit = {}\n", quote(&entry.commit)));
        out.push_str(&format!("hash = {}\n", quote(&entry.hash)));
        let targets: Vec<String> = entry.targets.iter().map(|t| quote(t)).collect();
        out.push_str(&format!("targets = [{}]\n", targets.join(", ")));
    }
    out
}

/// `path` に `content` をアトミックに書き込む（同じディレクトリの一時ファイルから rename する）。
///
/// パーミッションは既存ファイルがあればそれを引き継ぎ、なければ umask を適用した 0666 にする。
pub fn write(path: &Path, content: &str) -> Result<(), LockfileError> {
    let io_err = |err: io::Error| LockfileError::Io(path.to_path_buf(), err);
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut file = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o666))
        .tempfile_in(dir)
        .map_err(io_err)?;
    match fs::metadata(path) {
        Ok(metadata) => file
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(io_err)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(io_err(err)),
    }
    file.write_all(content.as_bytes()).map_err(io_err)?;
    file.persist(path).map_err(|err| io_err(err.error))?;
    Ok(())
}

fn is_commit_sha(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn quote(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Lockfile {
        Lockfile {
            skills: vec![
                Entry {
                    name: "pdf".into(),
                    github: "anthropics/skills".into(),
                    path: Some("skills/pdf".into()),
                    requested: Requested::Tag("v1.2.0".into()),
                    commit: "3f2a9d8c7b6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a".into(),
                    hash: "sha256-7b3e".into(),
                    targets: vec!["claude".into()],
                },
                Entry {
                    name: "my-review".into(),
                    github: "myuron/my-skills".into(),
                    path: None,
                    requested: Requested::Default,
                    commit: "9c1e4b7a2f3d8e6c5b4a39281706f5e4d3c2b1a0".into(),
                    hash: "sha256-2f8a".into(),
                    targets: vec!["claude".into(), "codex".into()],
                },
            ],
        }
    }

    #[test]
    fn renders_sorted_with_inline_requested() {
        assert_eq!(
            render(&sample()),
            r#"# このファイルは skill-lock が自動生成します。手で編集しないでください。
version = 1

[[skill]]
name = "my-review"
github = "myuron/my-skills"
requested = {}
commit = "9c1e4b7a2f3d8e6c5b4a39281706f5e4d3c2b1a0"
hash = "sha256-2f8a"
targets = ["claude", "codex"]

[[skill]]
name = "pdf"
github = "anthropics/skills"
path = "skills/pdf"
requested = { tag = "v1.2.0" }
commit = "3f2a9d8c7b6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a"
hash = "sha256-7b3e"
targets = ["claude"]
"#
        );
    }

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        write(&path, &render(&sample())).unwrap();
        let mut read_back = read(&path).unwrap().unwrap();
        let mut expected = sample();
        read_back.skills.sort_by(|a, b| a.name.cmp(&b.name));
        expected.skills.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(read_back, expected);
    }

    #[test]
    fn escapes_strings() {
        let mut lockfile = sample();
        lockfile.skills[0].requested = Requested::Branch("a\"b\\c".into());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        write(&path, &render(&lockfile)).unwrap();
        let read_back = read(&path).unwrap().unwrap();
        let pdf = read_back.skills.iter().find(|e| e.name == "pdf").unwrap();
        assert_eq!(pdf.requested, Requested::Branch("a\"b\\c".into()));
    }

    #[test]
    fn rejects_invalid_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        let mut lockfile = sample();
        lockfile.skills[0].commit = "あいう".into();
        write(&path, &render(&lockfile)).unwrap();
        assert!(matches!(read(&path), Err(LockfileError::Invalid(_, _))));
    }

    #[test]
    fn keeps_existing_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        for mode in [0o644, 0o640] {
            fs::write(&path, "old").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            write(&path, "new").unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), "new");
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                mode
            );
        }
    }

    #[test]
    fn new_file_follows_umask() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        write(&path, "new").unwrap();
        // fs::write と同じく 0666 に umask を適用したモードになる
        let reference = dir.path().join("reference");
        fs::write(&reference, "").unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), mode(&reference));
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&dir.path().join(LOCK_FILE)).unwrap(), None);
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        fs::write(&path, "version = 9\n").unwrap();
        assert!(matches!(
            read(&path),
            Err(LockfileError::UnsupportedVersion(_, 9))
        ));
    }
}
