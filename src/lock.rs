use std::fmt;
use std::fs;
use std::path::Path;

use crate::git::{FetchError, Fetcher, Spec};
use crate::hash::{self, File};
use crate::lockfile::{self, Entry, LOCK_FILE, Lockfile, LockfileError};
use crate::manifest::{self, ManifestError, Skill};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// 既存の lock エントリをそのまま使った
    Unchanged,
    /// 新たに解決した
    Locked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillReport {
    pub name: String,
    pub github: String,
    pub commit: String,
    pub status: Status,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// name の昇順
    pub skills: Vec<SkillReport>,
    /// マニフェストから削除され lock から除いた skill 名
    pub removed: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum LockError {
    Manifest(ManifestError),
    Lockfile(LockfileError),
    Fetch(String, FetchError),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Manifest(err) => err.fmt(f),
            LockError::Lockfile(err) => err.fmt(f),
            LockError::Fetch(name, err) => write!(f, "failed to lock skill {name}: {err}"),
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockError::Manifest(err) => Some(err),
            LockError::Lockfile(err) => Some(err),
            LockError::Fetch(_, err) => Some(err),
        }
    }
}

/// `dir` の `skill-lock.toml` を解決して `skill-lock.lock` を書き出す。
///
/// 既存の lock エントリのうち github / path / requested がマニフェストと一致するものは
/// そのまま使い、それ以外の skill だけを `fetcher` で解決する。
pub fn run(dir: &Path, fetcher: &impl Fetcher) -> Result<Report, LockError> {
    let manifest = manifest::load(dir).map_err(LockError::Manifest)?;
    let lock_path = dir.join(LOCK_FILE);
    let existing = lockfile::read(&lock_path)
        .map_err(LockError::Lockfile)?
        .unwrap_or_default();

    let mut report = Report::default();
    let mut entries = Vec::with_capacity(manifest.skills.len());
    for skill in &manifest.skills {
        let reusable = existing.skills.iter().find(|e| {
            e.name == skill.name
                && e.github == skill.github
                && e.path == skill.path
                && e.requested == skill.requested
        });
        let (entry, status) = match reusable {
            Some(entry) => (
                Entry {
                    targets: skill.targets.clone(),
                    ..entry.clone()
                },
                Status::Unchanged,
            ),
            None => (
                resolve(skill, fetcher, &mut report.warnings)?,
                Status::Locked,
            ),
        };
        report.skills.push(SkillReport {
            name: entry.name.clone(),
            github: entry.github.clone(),
            commit: entry.commit.clone(),
            status,
        });
        entries.push(entry);
    }
    report.removed = existing
        .skills
        .iter()
        .filter(|e| !manifest.skills.iter().any(|s| s.name == e.name))
        .map(|e| e.name.clone())
        .collect();

    let content = lockfile::render(&Lockfile { skills: entries });
    // 内容が変わらない場合は書き込まない
    if fs::read_to_string(&lock_path).ok().as_deref() != Some(content.as_str()) {
        lockfile::write(&lock_path, &content).map_err(LockError::Lockfile)?;
    }
    Ok(report)
}

fn resolve(
    skill: &Skill,
    fetcher: &impl Fetcher,
    warnings: &mut Vec<String>,
) -> Result<Entry, LockError> {
    let fetched = fetcher
        .fetch(&Spec {
            github: &skill.github,
            path: skill.path.as_deref(),
            requested: &skill.requested,
        })
        .map_err(|err| LockError::Fetch(skill.name.clone(), err))?;
    if let Some(name) = frontmatter_name(&fetched.files)
        && name != skill.name
    {
        warnings.push(format!(
            "skill {}: SKILL.md declares name {name:?}, which differs from the skill name",
            skill.name
        ));
    }
    Ok(Entry {
        name: skill.name.clone(),
        github: skill.github.clone(),
        path: skill.path.clone(),
        requested: skill.requested.clone(),
        commit: fetched.commit,
        hash: hash::hash(&fetched.files),
        targets: skill.targets.clone(),
    })
}

/// SKILL.md の YAML frontmatter から `name` を取り出す。
fn frontmatter_name(files: &[File]) -> Option<String> {
    let skill = files.iter().find(|f| f.path == "SKILL.md")?;
    let text = std::str::from_utf8(&skill.content).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    lines
        .take_while(|line| line.trim_end() != "---")
        .find_map(|line| line.strip_prefix("name:"))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use crate::git::Fetched;
    use crate::manifest::{MANIFEST_FILE, Requested};

    /// 呼び出しを記録し、固定の内容を返すフェイク。
    #[derive(Default)]
    struct FakeFetcher {
        calls: RefCell<Vec<String>>,
        skill_md: &'static str,
    }

    impl Fetcher for FakeFetcher {
        fn fetch(&self, spec: &Spec) -> Result<Fetched, FetchError> {
            self.calls.borrow_mut().push(spec.github.to_string());
            Ok(Fetched {
                commit: format!("{:0<40}", spec.github.len()),
                files: vec![File {
                    path: "SKILL.md".into(),
                    executable: false,
                    content: self.skill_md.as_bytes().to_vec(),
                }],
            })
        }
    }

    fn setup(manifest: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(MANIFEST_FILE), manifest).unwrap();
        dir
    }

    fn read_lock(dir: &Path) -> Lockfile {
        lockfile::read(&dir.join(LOCK_FILE)).unwrap().unwrap()
    }

    const TWO_SKILLS: &str = r#"
version = 1
[skills.a]
github = "o/a"
tag = "v1"
[skills.b]
github = "o/bb"
path = "skills/b"
"#;

    #[test]
    fn creates_lockfile() {
        let dir = setup(TWO_SKILLS);
        let fetcher = FakeFetcher::default();
        let report = run(dir.path(), &fetcher).unwrap();
        assert_eq!(*fetcher.calls.borrow(), ["o/a", "o/bb"]);
        assert!(report.skills.iter().all(|s| s.status == Status::Locked));

        let lock = read_lock(dir.path());
        assert_eq!(lock.skills.len(), 2);
        assert_eq!(lock.skills[0].name, "a");
        assert_eq!(lock.skills[0].requested, Requested::Tag("v1".into()));
        assert_eq!(lock.skills[1].path.as_deref(), Some("skills/b"));
        assert_eq!(lock.skills[1].targets, ["claude"]);
        assert!(lock.skills[1].hash.starts_with("sha256-"));
    }

    #[test]
    fn reuses_matching_entries() {
        let dir = setup(TWO_SKILLS);
        run(dir.path(), &FakeFetcher::default()).unwrap();
        let before = fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap();

        let fetcher = FakeFetcher::default();
        let report = run(dir.path(), &fetcher).unwrap();
        assert!(fetcher.calls.borrow().is_empty());
        assert!(report.skills.iter().all(|s| s.status == Status::Unchanged));
        assert_eq!(
            fs::read_to_string(dir.path().join(LOCK_FILE)).unwrap(),
            before
        );
    }

    #[test]
    fn re_resolves_only_changed_entries_and_drops_removed() {
        let dir = setup(TWO_SKILLS);
        run(dir.path(), &FakeFetcher::default()).unwrap();

        fs::write(
            dir.path().join(MANIFEST_FILE),
            r#"
version = 1
[skills.a]
github = "o/a"
tag = "v2"
[skills.c]
github = "o/ccc"
"#,
        )
        .unwrap();
        let fetcher = FakeFetcher::default();
        let report = run(dir.path(), &fetcher).unwrap();
        assert_eq!(*fetcher.calls.borrow(), ["o/a", "o/ccc"]);
        assert_eq!(report.removed, ["b"]);

        let lock = read_lock(dir.path());
        let names: Vec<&str> = lock.skills.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
        assert_eq!(lock.skills[0].requested, Requested::Tag("v2".into()));
    }

    #[test]
    fn updates_targets_without_fetching() {
        let dir = setup(TWO_SKILLS);
        run(dir.path(), &FakeFetcher::default()).unwrap();
        fs::write(
            dir.path().join(MANIFEST_FILE),
            TWO_SKILLS.replace("tag = \"v1\"", "tag = \"v1\"\ntargets = [\"codex\"]"),
        )
        .unwrap();

        let fetcher = FakeFetcher::default();
        run(dir.path(), &fetcher).unwrap();
        assert!(fetcher.calls.borrow().is_empty());
        assert_eq!(read_lock(dir.path()).skills[0].targets, ["codex"]);
    }

    #[test]
    fn warns_on_frontmatter_name_mismatch() {
        let dir = setup(TWO_SKILLS);
        let fetcher = FakeFetcher {
            skill_md: "---\nname: \"a\"\ndescription: x\n---\nbody\n",
            ..Default::default()
        };
        let report = run(dir.path(), &fetcher).unwrap();
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].starts_with("skill b:"));
    }

    #[test]
    fn fails_without_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(dir.path(), &FakeFetcher::default()).unwrap_err();
        assert!(matches!(
            err,
            LockError::Manifest(ManifestError::NotFound(_))
        ));
        assert!(!dir.path().join(LOCK_FILE).exists());
    }

    #[test]
    fn locks_with_git_fetcher() {
        let repo = crate::git::tests::TestRepo::new("owner/repo");
        repo.write("skills/pdf/SKILL.md", "---\nname: pdf\n---\n");
        let commit = repo.commit("init");
        let dir =
            setup("version = 1\n[skills.pdf]\ngithub = \"owner/repo\"\npath = \"skills/pdf\"\n");

        let report = run(dir.path(), &repo.fetcher()).unwrap();
        assert!(report.warnings.is_empty());
        let lock = read_lock(dir.path());
        assert_eq!(lock.skills[0].commit, commit);
        assert_eq!(
            lock.skills[0].hash,
            hash::hash(&[File {
                path: "SKILL.md".into(),
                executable: false,
                content: b"---\nname: pdf\n---\n".to_vec(),
            }])
        );
    }
}
