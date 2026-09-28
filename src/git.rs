use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use crate::hash::File;
use crate::manifest::Requested;

const GITHUB_BASE_URL: &str = "https://github.com/";
const SKILL_FILE: &str = "SKILL.md";

/// 一時リポジトリ以外を操作しないように取り除く、リポジトリの場所を指す環境変数。
/// `git rev-parse --local-env-vars` のうち、認証などの設定を渡す `GIT_CONFIG*` は残す。
const REPO_ENV_VARS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// 取得する skill の指定。
#[derive(Debug, Clone, Copy)]
pub struct Spec<'a> {
    /// `owner/repo`
    pub github: &'a str,
    pub path: Option<&'a str>,
    pub requested: &'a Requested,
}

/// 取得結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// 解決された 40 桁のコミット SHA
    pub commit: String,
    /// skill ディレクトリ内の全ファイル
    pub files: Vec<File>,
}

#[derive(Debug)]
pub struct FetchError(String);

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FetchError {}

pub trait Fetcher {
    fn fetch(&self, spec: &Spec) -> Result<Fetched, FetchError>;
}

/// git CLI で skill を取得する。認証は git の設定（credential helper・SSH など）に任せる。
pub struct GitFetcher {
    base_url: String,
}

impl GitFetcher {
    pub fn github() -> Self {
        Self::with_base_url(GITHUB_BASE_URL)
    }

    /// `<base_url><owner>/<repo>.git` から取得する。
    pub fn with_base_url(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }
}

impl Fetcher for GitFetcher {
    fn fetch(&self, spec: &Spec) -> Result<Fetched, FetchError> {
        let url = format!("{}{}.git", self.base_url, spec.github);
        let refspec = match spec.requested {
            Requested::Default => "HEAD".to_string(),
            Requested::Tag(tag) => format!("refs/tags/{tag}"),
            Requested::Branch(branch) => format!("refs/heads/{branch}"),
            Requested::Rev(rev) if rev.len() == 40 => rev.to_ascii_lowercase(),
            Requested::Rev(rev) => resolve_short_rev(&url, rev)?,
        };

        let repo = Repo::init(&url)?;
        repo.run(&[
            "fetch",
            "--quiet",
            "--no-tags",
            "--depth",
            "1",
            "origin",
            &refspec,
        ])
        .map_err(|err| FetchError(format!("failed to fetch {refspec} from {url}: {err}")))?;
        let commit = repo.rev_parse("FETCH_HEAD^{commit}")?;
        let files = repo.read_dir(&commit, spec.path)?;
        if !files.iter().any(|f| f.path == SKILL_FILE) {
            return Err(FetchError(format!(
                "{SKILL_FILE} not found in {}{} at {commit}",
                spec.github,
                spec.path.map(|p| format!("/{p}")).unwrap_or_default()
            )));
        }
        Ok(Fetched { commit, files })
    }
}

/// 短縮形のコミット SHA を 40 桁に解決する。ref を blob なしで全取得してから探す。
fn resolve_short_rev(url: &str, rev: &str) -> Result<String, FetchError> {
    let repo = Repo::init(url)?;
    repo.run(&[
        "fetch",
        "--quiet",
        "--filter=blob:none",
        "origin",
        "+refs/heads/*:refs/remotes/origin/*",
        "+refs/tags/*:refs/tags/*",
    ])
    .map_err(|err| FetchError(format!("failed to fetch refs from {url}: {err}")))?;
    repo.rev_parse(&format!("{rev}^{{commit}}"))
        .map_err(|err| FetchError(format!("failed to resolve rev {rev} in {url}: {err}")))
}

/// 取得用の一時リポジトリ。
struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn init(url: &str) -> Result<Self, FetchError> {
        let dir = tempfile::tempdir()
            .map_err(|err| FetchError(format!("failed to create a temporary directory: {err}")))?;
        let repo = Repo { dir };
        repo.run(&["init", "--quiet"])?;
        repo.run(&["remote", "add", "origin", url])?;
        Ok(repo)
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(self.dir.path())
            .arg("--literal-pathspecs");
        for var in REPO_ENV_VARS {
            cmd.env_remove(var);
        }
        cmd
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, FetchError> {
        run(self.command().args(args), args)
    }

    fn rev_parse(&self, rev: &str) -> Result<String, FetchError> {
        let out = self.run(&["rev-parse", "--verify", "--quiet", rev])?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    /// `commit` の `path` 以下の全ファイルを読み出す。パスは `path` からの相対パスになる。
    fn read_dir(&self, commit: &str, path: Option<&str>) -> Result<Vec<File>, FetchError> {
        let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", commit];
        if let Some(path) = path {
            args.extend(["--", path]);
        }
        let listing = self.run(&args)?;
        let prefix = path.map(|p| format!("{p}/")).unwrap_or_default();

        let mut entries = Vec::new();
        for record in listing.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            let record = String::from_utf8(record.to_vec())
                .map_err(|_| FetchError("non UTF-8 file name in repository".to_string()))?;
            let (meta, full_path) = record
                .split_once('\t')
                .ok_or_else(|| FetchError(format!("unexpected ls-tree output: {record:?}")))?;
            let mut fields = meta.split(' ');
            let (Some(mode), Some(_kind), Some(oid)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(FetchError(format!("unexpected ls-tree output: {record:?}")));
            };
            // path がファイルを指している場合は prefix に一致しない
            let Some(rel) = full_path.strip_prefix(&prefix) else {
                continue;
            };
            let executable = match mode {
                "100644" => false,
                "100755" => true,
                "120000" => {
                    return Err(FetchError(format!(
                        "symbolic links are not supported: {full_path}"
                    )));
                }
                "160000" => {
                    return Err(FetchError(format!(
                        "submodules are not supported: {full_path}"
                    )));
                }
                _ => {
                    return Err(FetchError(format!(
                        "unsupported file mode {mode}: {full_path}"
                    )));
                }
            };
            entries.push((rel.to_string(), executable, oid.to_string()));
        }

        let oids: Vec<&str> = entries.iter().map(|(_, _, oid)| oid.as_str()).collect();
        let contents = self.cat_blobs(&oids)?;
        Ok(entries
            .into_iter()
            .zip(contents)
            .map(|((path, executable, _), content)| File {
                path,
                executable,
                content,
            })
            .collect())
    }

    /// `git cat-file --batch` で blob の内容をまとめて読む。
    fn cat_blobs(&self, oids: &[&str]) -> Result<Vec<Vec<u8>>, FetchError> {
        if oids.is_empty() {
            return Ok(Vec::new());
        }
        let io_err = |err: io::Error| FetchError(format!("failed to run git cat-file: {err}"));
        let mut child = self
            .command()
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(io_err)?;

        let input: String = oids.iter().map(|oid| format!("{oid}\n")).collect();
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));

        let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut contents = Vec::with_capacity(oids.len());
        for oid in oids {
            let mut header = String::new();
            reader.read_line(&mut header).map_err(io_err)?;
            let size = match header.trim_end().split(' ').collect::<Vec<_>>()[..] {
                [_, "blob", size] => size.parse::<usize>().ok(),
                _ => None,
            }
            .ok_or_else(|| FetchError(format!("failed to read blob {oid}: {}", header.trim())))?;
            let mut content = vec![0; size + 1]; // 末尾の改行を含む
            reader.read_exact(&mut content).map_err(io_err)?;
            content.pop();
            contents.push(content);
        }
        drop(reader);

        writer
            .join()
            .expect("writer thread panicked")
            .map_err(io_err)?;
        let output = child.wait_with_output().map_err(io_err)?;
        if !output.status.success() {
            return Err(FetchError(format!(
                "git cat-file failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(contents)
    }
}

fn run(cmd: &mut Command, args: &[&str]) -> Result<Vec<u8>, FetchError> {
    let output = cmd.output().map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => FetchError("git command not found".to_string()),
        _ => FetchError(format!("failed to run git: {err}")),
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FetchError(format!(
            "`git {}` failed: {}",
            args.join(" "),
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// テスト用のローカルリポジトリ。`<root>/<owner>/<repo>.git` に作る。
    pub(crate) struct TestRepo {
        pub(crate) root: tempfile::TempDir,
        dir: PathBuf,
    }

    impl TestRepo {
        pub(crate) fn new(github: &str) -> Self {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(format!("{github}.git"));
            fs::create_dir_all(&dir).unwrap();
            let repo = TestRepo { root, dir };
            repo.git(&["init", "--quiet", "--initial-branch=main"]);
            repo
        }

        pub(crate) fn fetcher(&self) -> GitFetcher {
            GitFetcher::with_base_url(&format!("file://{}/", self.root.path().display()))
        }

        pub(crate) fn git(&self, args: &[&str]) -> String {
            let out = run(
                Command::new("git")
                    .arg("-C")
                    .arg(&self.dir)
                    .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
                    .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
                    .args(args),
                args,
            )
            .unwrap();
            String::from_utf8(out).unwrap().trim().to_string()
        }

        pub(crate) fn write(&self, path: &str, content: &str) {
            let path = self.dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        pub(crate) fn set_executable(&self, path: &str) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(self.dir.join(path), fs::Permissions::from_mode(0o755)).unwrap();
        }

        pub(crate) fn commit(&self, message: &str) -> String {
            self.git(&["add", "--all"]);
            self.git(&["commit", "--quiet", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }
    }

    fn fetch(
        repo: &TestRepo,
        path: Option<&str>,
        requested: Requested,
    ) -> Result<Fetched, FetchError> {
        repo.fetcher().fetch(&Spec {
            github: "owner/repo",
            path,
            requested: &requested,
        })
    }

    fn paths(fetched: &Fetched) -> Vec<&str> {
        let mut paths: Vec<&str> = fetched.files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        paths
    }

    #[test]
    fn fetches_default_branch_subdirectory() {
        let repo = TestRepo::new("owner/repo");
        repo.write("README.md", "readme");
        repo.write("skills/pdf/SKILL.md", "---\nname: pdf\n---\n");
        repo.write("skills/pdf/scripts/run.sh", "#!/bin/sh\n");
        repo.write("skills/pdf-extra/SKILL.md", "other");
        repo.set_executable("skills/pdf/scripts/run.sh");
        let commit = repo.commit("init");

        let fetched = fetch(&repo, Some("skills/pdf"), Requested::Default).unwrap();
        assert_eq!(fetched.commit, commit);
        assert_eq!(paths(&fetched), ["SKILL.md", "scripts/run.sh"]);
        let run = fetched
            .files
            .iter()
            .find(|f| f.path == "scripts/run.sh")
            .unwrap();
        assert!(run.executable);
        assert_eq!(run.content, b"#!/bin/sh\n");
        let skill = fetched.files.iter().find(|f| f.path == "SKILL.md").unwrap();
        assert!(!skill.executable);
    }

    #[test]
    fn fetches_repository_root() {
        let repo = TestRepo::new("owner/repo");
        repo.write("SKILL.md", "root");
        repo.write("a/b.txt", "b");
        repo.commit("init");
        let fetched = fetch(&repo, None, Requested::Default).unwrap();
        assert_eq!(paths(&fetched), ["SKILL.md", "a/b.txt"]);
    }

    #[test]
    fn resolves_branch_tag_and_revs() {
        let repo = TestRepo::new("owner/repo");
        repo.write("SKILL.md", "v1");
        let first = repo.commit("first");
        repo.git(&["tag", "light"]);
        repo.git(&["tag", "-a", "annotated", "-m", "annotated"]);
        repo.git(&["checkout", "--quiet", "-b", "dev"]);
        repo.write("SKILL.md", "dev");
        let dev = repo.commit("dev");
        repo.git(&["checkout", "--quiet", "main"]);
        repo.write("SKILL.md", "v2");
        let second = repo.commit("second");

        let commit = |requested| fetch(&repo, None, requested).unwrap().commit;
        assert_eq!(commit(Requested::Default), second);
        assert_eq!(commit(Requested::Branch("dev".into())), dev);
        assert_eq!(commit(Requested::Tag("light".into())), first);
        assert_eq!(commit(Requested::Tag("annotated".into())), first);
        assert_eq!(commit(Requested::Rev(first.clone())), first);
        assert_eq!(commit(Requested::Rev(first[..7].to_string())), first);

        let content = fetch(&repo, None, Requested::Branch("dev".into())).unwrap();
        assert_eq!(content.files[0].content, b"dev");
    }

    #[test]
    fn fails_without_skill_md() {
        let repo = TestRepo::new("owner/repo");
        repo.write("skills/pdf/README.md", "x");
        repo.commit("init");
        let err = fetch(&repo, Some("skills/pdf"), Requested::Default).unwrap_err();
        assert!(err.to_string().contains("SKILL.md not found"), "{err}");
        let err = fetch(&repo, Some("missing"), Requested::Default).unwrap_err();
        assert!(err.to_string().contains("SKILL.md not found"), "{err}");
    }

    #[test]
    fn fails_on_symlink() {
        let repo = TestRepo::new("owner/repo");
        repo.write("SKILL.md", "x");
        std::os::unix::fs::symlink("SKILL.md", repo.dir.join("link")).unwrap();
        repo.commit("init");
        let err = fetch(&repo, None, Requested::Default).unwrap_err();
        assert!(err.to_string().contains("symbolic links"), "{err}");
    }

    #[test]
    fn clears_repository_env_vars() {
        let repo = Repo::init("file:///unused").unwrap();
        let cmd = repo.command();
        let removed: Vec<_> = cmd
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_str().unwrap())
            .collect();
        for var in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
        ] {
            assert!(removed.contains(&var), "{var} is not removed");
        }
    }

    #[test]
    fn fails_on_unknown_ref() {
        let repo = TestRepo::new("owner/repo");
        repo.write("SKILL.md", "x");
        repo.commit("init");
        assert!(fetch(&repo, None, Requested::Branch("nope".into())).is_err());
        assert!(fetch(&repo, None, Requested::Rev("deadbeef".into())).is_err());
    }
}
