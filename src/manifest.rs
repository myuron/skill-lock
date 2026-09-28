use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

pub const MANIFEST_FILE: &str = "skill-lock.toml";

const SUPPORTED_VERSION: u32 = 1;
const DEFAULT_TARGETS: &[&str] = &["claude"];
const BUILTIN_TARGETS: &[(&str, &str)] =
    &[("claude", ".claude/skills"), ("codex", ".agents/skills")];

/// マニフェストで指定された ref。lock ファイルの `requested` と共通。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawRequested")]
pub enum Requested {
    /// 指定なし（リポジトリのデフォルトブランチ）
    #[default]
    Default,
    Tag(String),
    Branch(String),
    Rev(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequested {
    tag: Option<String>,
    branch: Option<String>,
    rev: Option<String>,
}

impl TryFrom<RawRequested> for Requested {
    type Error = String;

    fn try_from(raw: RawRequested) -> Result<Self, Self::Error> {
        match (raw.tag, raw.branch, raw.rev) {
            (None, None, None) => Ok(Requested::Default),
            (Some(tag), None, None) => Ok(Requested::Tag(tag)),
            (None, Some(branch), None) => Ok(Requested::Branch(branch)),
            (None, None, Some(rev)) => Ok(Requested::Rev(rev)),
            _ => Err("only one of `tag`, `branch` and `rev` can be specified".to_string()),
        }
    }
}

/// 検証済みのマニフェスト。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// name の昇順
    pub skills: Vec<Skill>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    /// `owner/repo`
    pub github: String,
    /// リポジトリ内の skill ディレクトリ。`None` はリポジトリルート
    pub path: Option<String>,
    pub requested: Requested,
    pub targets: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    version: u32,
    #[serde(default)]
    settings: RawSettings,
    #[serde(default)]
    targets: BTreeMap<String, RawTarget>,
    #[serde(default)]
    skills: BTreeMap<String, RawSkill>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    targets: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTarget {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSkill {
    github: Option<String>,
    path: Option<String>,
    tag: Option<String>,
    branch: Option<String>,
    rev: Option<String>,
    targets: Option<Vec<String>>,
}

#[derive(Debug)]
pub enum ManifestError {
    NotFound(PathBuf),
    Io(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
    Invalid(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::NotFound(path) => write!(
                f,
                "{} not found (run `skill-lock init` to create it)",
                path.display()
            ),
            ManifestError::Io(path, err) => write!(f, "failed to read {}: {err}", path.display()),
            ManifestError::Parse(path, err) => {
                write!(f, "failed to parse {}: {err}", path.display())
            }
            ManifestError::Invalid(msg) => write!(f, "invalid {MANIFEST_FILE}: {msg}"),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ManifestError::Io(_, err) => Some(err),
            ManifestError::Parse(_, err) => Some(err),
            ManifestError::NotFound(_) | ManifestError::Invalid(_) => None,
        }
    }
}

/// `dir` の `skill-lock.toml` を読み込んで検証する。
pub fn load(dir: &Path) -> Result<Manifest, ManifestError> {
    let path = dir.join(MANIFEST_FILE);
    let text = fs::read_to_string(&path).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => ManifestError::NotFound(path.clone()),
        _ => ManifestError::Io(path.clone(), err),
    })?;
    let raw: RawManifest =
        toml::from_str(&text).map_err(|err| ManifestError::Parse(path.clone(), err))?;
    validate(raw).map_err(ManifestError::Invalid)
}

fn validate(raw: RawManifest) -> Result<Manifest, String> {
    if raw.version != SUPPORTED_VERSION {
        return Err(format!(
            "unsupported version {} (supported: {SUPPORTED_VERSION})",
            raw.version
        ));
    }

    for (name, target) in &raw.targets {
        if !is_relative_inside(&target.path) {
            return Err(format!(
                "targets.{name}.path must be a relative path inside the project: {:?}",
                target.path
            ));
        }
    }
    let target_defined = |name: &str| {
        raw.targets.contains_key(name) || BUILTIN_TARGETS.iter().any(|(n, _)| *n == name)
    };

    let default_targets = match raw.settings.targets {
        Some(targets) => targets,
        None => DEFAULT_TARGETS.iter().map(|t| t.to_string()).collect(),
    };
    if let Some(name) = default_targets.iter().find(|t| !target_defined(t)) {
        return Err(format!(
            "settings.targets refers to undefined target {name:?}"
        ));
    }

    let mut skills = Vec::with_capacity(raw.skills.len());
    for (name, skill) in raw.skills {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(format!("skill name {name:?} must match [a-z0-9-]+"));
        }
        let github = skill
            .github
            .ok_or_else(|| format!("skills.{name}: `github` is required"))?;
        if !is_owner_repo(&github) {
            return Err(format!(
                "skills.{name}.github must be in `owner/repo` form: {github:?}"
            ));
        }
        if let Some(path) = &skill.path
            && !is_repo_subpath(path)
        {
            return Err(format!(
                "skills.{name}.path must be a normalized relative path like `skills/pdf`: {path:?}"
            ));
        }
        let requested = Requested::try_from(RawRequested {
            tag: skill.tag,
            branch: skill.branch,
            rev: skill.rev,
        })
        .map_err(|msg| format!("skills.{name}: {msg}"))?;
        if let Requested::Rev(rev) = &requested
            && !((4..=40).contains(&rev.len()) && rev.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(format!(
                "skills.{name}.rev must be a 4 to 40 digit hex commit SHA: {rev:?}"
            ));
        }
        let targets = skill.targets.unwrap_or_else(|| default_targets.clone());
        if let Some(target) = targets.iter().find(|t| !target_defined(t)) {
            return Err(format!(
                "skills.{name}.targets refers to undefined target {target:?}"
            ));
        }
        skills.push(Skill {
            name,
            github,
            path: skill.path,
            requested,
            targets,
        });
    }

    Ok(Manifest { skills })
}

/// プロジェクトルート内を指す相対パスか。
fn is_relative_inside(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn is_owner_repo(github: &str) -> bool {
    let valid = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    match github.split_once('/') {
        Some((owner, repo)) => valid(owner) && valid(repo),
        None => false,
    }
}

/// `a/b/c` 形式（空・`.`・`..` のセグメントや先頭・末尾の `/` を含まない）か。
fn is_repo_subpath(path: &str) -> bool {
    path.split('/')
        .all(|s| !s.is_empty() && s != "." && s != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Manifest, String> {
        let raw: RawManifest = toml::from_str(text).map_err(|e| e.to_string())?;
        validate(raw)
    }

    fn assert_invalid(text: &str, needle: &str) {
        let err = parse(text).unwrap_err();
        assert!(err.contains(needle), "{err:?} does not contain {needle:?}");
    }

    #[test]
    fn parses_full_example() {
        let manifest = parse(
            r#"
version = 1

[settings]
targets = ["claude"]

[targets.shared]
path = "tools/skills"

[skills.pdf]
github = "anthropics/skills"
path = "skills/pdf"
tag = "v1.2.0"

[skills.my-review]
github = "myuron/my-skills"
path = "review"
branch = "main"
targets = ["claude", "codex", "shared"]

[skills.pinned]
github = "myuron/pinned"
rev = "abc1234"
"#,
        )
        .unwrap();
        assert_eq!(
            manifest.skills,
            vec![
                Skill {
                    name: "my-review".into(),
                    github: "myuron/my-skills".into(),
                    path: Some("review".into()),
                    requested: Requested::Branch("main".into()),
                    targets: vec!["claude".into(), "codex".into(), "shared".into()],
                },
                Skill {
                    name: "pdf".into(),
                    github: "anthropics/skills".into(),
                    path: Some("skills/pdf".into()),
                    requested: Requested::Tag("v1.2.0".into()),
                    targets: vec!["claude".into()],
                },
                Skill {
                    name: "pinned".into(),
                    github: "myuron/pinned".into(),
                    path: None,
                    requested: Requested::Rev("abc1234".into()),
                    targets: vec!["claude".into()],
                },
            ]
        );
    }

    #[test]
    fn defaults_targets_to_claude() {
        let manifest = parse("version = 1\n[skills.a]\ngithub = \"o/r\"\n").unwrap();
        assert_eq!(manifest.skills[0].targets, vec!["claude".to_string()]);
        assert_eq!(manifest.skills[0].requested, Requested::Default);
    }

    #[test]
    fn rejects_missing_or_unsupported_version() {
        assert_invalid("", "version");
        assert_invalid("version = 2", "unsupported version");
    }

    #[test]
    fn rejects_unknown_keys() {
        assert_invalid("version = 1\nfoo = 1", "foo");
        assert_invalid(
            "version = 1\n[skills.a]\ngithub = \"o/r\"\ntags = \"v1\"",
            "tags",
        );
    }

    #[test]
    fn rejects_multiple_refs() {
        assert_invalid(
            "version = 1\n[skills.a]\ngithub = \"o/r\"\ntag = \"v1\"\nbranch = \"main\"",
            "only one of",
        );
    }

    #[test]
    fn rejects_invalid_skill_fields() {
        assert_invalid(
            "version = 1\n[skills.a]\npath = \"x\"",
            "`github` is required",
        );
        assert_invalid("version = 1\n[skills.a]\ngithub = \"repo\"", "owner/repo");
        assert_invalid("version = 1\n[skills.a]\ngithub = \"o/r/x\"", "owner/repo");
        assert_invalid("version = 1\n[skills.Bad]\ngithub = \"o/r\"", "[a-z0-9-]+");
        assert_invalid(
            "version = 1\n[skills.a]\ngithub = \"o/r\"\npath = \"../x\"",
            "normalized relative path",
        );
        assert_invalid(
            "version = 1\n[skills.a]\ngithub = \"o/r\"\npath = \"x/\"",
            "normalized relative path",
        );
        assert_invalid(
            "version = 1\n[skills.a]\ngithub = \"o/r\"\nrev = \"xyz\"",
            "hex commit SHA",
        );
    }

    #[test]
    fn rejects_undefined_targets() {
        assert_invalid(
            "version = 1\n[settings]\ntargets = [\"nope\"]",
            "undefined target",
        );
        assert_invalid(
            "version = 1\n[skills.a]\ngithub = \"o/r\"\ntargets = [\"nope\"]",
            "undefined target",
        );
    }

    #[test]
    fn rejects_target_paths_outside_project() {
        assert_invalid(
            "version = 1\n[targets.x]\npath = \"/abs\"",
            "inside the project",
        );
        assert_invalid(
            "version = 1\n[targets.x]\npath = \"a/../..\"",
            "inside the project",
        );
    }

    #[test]
    fn load_reports_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(load(dir.path()), Err(ManifestError::NotFound(_))));
    }
}
