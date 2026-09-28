use sha2::{Digest, Sha256};

/// skill ディレクトリ内の 1 ファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// skill ディレクトリからの `/` 区切りの相対パス
    pub path: String,
    pub executable: bool,
    pub content: Vec<u8>,
}

/// skill ディレクトリの内容ハッシュを `sha256-<16進数>` の形式で返す。
///
/// ファイルを相対パスのバイト順にソートし、各ファイルを
/// `<path>\0<1|0>\0<内容の長さ(10進)>\0<内容>` として連結したものの SHA-256 を取る。
pub fn hash(files: &[File]) -> String {
    let mut sorted: Vec<&File> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));

    let mut hasher = Sha256::new();
    for file in sorted {
        hasher.update(file.path.as_bytes());
        hasher.update(b"\0");
        hasher.update(if file.executable { b"1" } else { b"0" });
        hasher.update(b"\0");
        hasher.update(file.content.len().to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(&file.content);
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256-{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, executable: bool, content: &str) -> File {
        File {
            path: path.to_string(),
            executable,
            content: content.as_bytes().to_vec(),
        }
    }

    #[test]
    fn matches_expected_value() {
        let files = [file("SKILL.md", false, "hello")];
        // printf 'SKILL.md\x000\x005\x00hello' | sha256sum
        assert_eq!(
            hash(&files),
            "sha256-c74a5ff3c989dfa66618c2f5faa080c8c85fdd74efda75e4121d98718335f07a"
        );
    }

    #[test]
    fn independent_of_input_order() {
        let a = [file("a", false, "1"), file("b/c", false, "2")];
        let b = [file("b/c", false, "2"), file("a", false, "1")];
        assert_eq!(hash(&a), hash(&b));
    }

    #[test]
    fn changes_with_executable_bit() {
        assert_ne!(
            hash(&[file("run.sh", false, "x")]),
            hash(&[file("run.sh", true, "x")])
        );
    }

    #[test]
    fn changes_with_content_and_path() {
        let base = hash(&[file("a", false, "1")]);
        assert_ne!(base, hash(&[file("a", false, "2")]));
        assert_ne!(base, hash(&[file("b", false, "1")]));
    }

    #[test]
    fn file_boundaries_are_unambiguous() {
        assert_ne!(
            hash(&[file("a", false, "bc")]),
            hash(&[file("a", false, "b"), file("c", false, "")])
        );
    }
}
