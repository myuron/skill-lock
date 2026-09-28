# レビュー票: PR #3 Add lock subcommand

| 項目 | 内容 |
| --- | --- |
| PR | [#3](https://github.com/myuron/skill-lock/pull/3) |
| ブランチ | `feat/lock-subcommand` |
| レビュー日 | 2026-09-28 |
| レビュー方法 | `/code-review`（Claude Code） |

## 指摘事項

### 1. 継承した git 環境変数により、利用者のリポジトリを操作してしまう

| 項目 | 内容 |
| --- | --- |
| 箇所 | `src/git.rs:128`（`Repo::command`） |
| 重要度 | 中 |
| 分類 | 正確性 |
| 状態 | 対応済み |

**内容**

git コマンドを `git -C <一時ディレクトリ>` で実行しているが、親プロセスから継承した `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE` をクリアしていない。`-C` は `GIT_DIR` を上書きしないため、`GIT_DIR=<repo>/.git git -C /tmp rev-parse --git-dir` は外側のリポジトリを返す。

**影響**

git hook 内や `GIT_DIR` を export したスクリプトから `skill-lock lock` を実行すると、`init` / `remote add origin` / `fetch` / `ls-tree` が一時リポジトリではなく利用者のリポジトリに対して実行される。`remote add origin` が既存の origin と衝突して失敗するか、取得したオブジェクトや `FETCH_HEAD` が利用者のリポジトリに書き込まれる。

**修正方針**

`Repo::command()` で `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE` を `env_remove` する。

**対応内容**

- `Repo::command()` で、`git rev-parse --local-env-vars` が返すリポジトリの場所を指す環境変数（`GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE` / `GIT_COMMON_DIR` など）を `env_remove` するようにした。
- 認証などの設定を渡すのに使われる `GIT_CONFIG_PARAMETERS` / `GIT_CONFIG_COUNT` / `GIT_CONFIG` は残した。
- 除去されることを確認するテスト `clears_repository_env_vars` を追加した。`GIT_DIR` に別リポジトリを指定した状態でも `lock` が成功し、そのリポジトリが変更されないことを手動で確認した。

### 2. lock ファイルのパーミッションが 0600 になる

| 項目 | 内容 |
| --- | --- |
| 箇所 | `src/lockfile.rs:127`（`lockfile::write`） |
| 重要度 | 低 |
| 分類 | 正確性 |
| 状態 | 対応済み |

**内容**

アトミックな書き込みのため `tempfile::NamedTempFile` を作成して rename しているが、`NamedTempFile` はモード 0600 でファイルを作成する。

**影響**

内容が変わる `lock` を実行するたびに、umask や元のパーミッションに関係なく `skill-lock.lock` が 0600 になる。別ユーザー・別 UID で動く CI コンテナ・共有チェックアウトから読めなくなる。

**修正方針**

rename の前に、既存ファイルがあればそのパーミッションを、なければ umask を適用した 0644 を一時ファイルに設定する。

**対応内容**

- 一時ファイルを `tempfile::Builder::permissions(0o666)` で作成し、umask が適用されるようにした（`fs::write` で新規作成した場合と同じモード）。
- 既存の lock ファイルがある場合は、そのパーミッションを一時ファイルに設定してから rename するようにした。
- テスト `keeps_existing_permissions`（0644 / 0640 が維持される）と `new_file_follows_umask` を追加した。

### 3. 不正な lock の `commit` で panic する

| 項目 | 内容 |
| --- | --- |
| 箇所 | `src/main.rs:52`（`&skill.commit[..7.min(skill.commit.len())]`） |
| 重要度 | 低 |
| 分類 | 堅牢性 |
| 状態 | 対応済み |

**内容**

commit の短縮表示をバイト単位のスライスで行っている。既存の lock エントリを再利用する場合、`commit` は lock ファイルの値がそのまま使われ、検証されていない。

**影響**

手で編集された・壊れた lock の `commit` の先頭 7 バイト以内にマルチバイト文字があると、エラーを報告する代わりに文字境界のスライスで panic する。

**修正方針**

lock 読み込み時に `commit` が 40 桁の 16 進数であることを検証し、不正な場合はエラーにする。

**対応内容**

- `lockfile::read` で各エントリの `commit` が 40 桁の小文字 16 進数であることを検証し、不正な場合は `LockfileError::Invalid` を返すようにした。
- テスト `rejects_invalid_commit` を追加した。

## 問題なしと判断した点

- 修正前の時点でテスト 35 件がすべて通過していた（修正後は 39 件）。
- 上記以外に、lock サブコマンドの差分（マニフェスト検証・lock の再利用判定・ハッシュ計算・git による取得）で指摘事項はなかった。
