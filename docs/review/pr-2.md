# レビュー票: PR #2 Add init subcommand

| 項目 | 内容 |
| --- | --- |
| PR | [#2](https://github.com/myuron/skill-lock/pull/2) |
| ブランチ | `feat/init-subcommand` |
| レビュー日 | 2026-09-27 |
| レビュー方法 | `/code-review`（Claude Code） |

## 指摘事項

### 1. 書き込み失敗時にファイルが残り、`init` を再実行できない

| 項目 | 内容 |
| --- | --- |
| 箇所 | `src/init.rs:54`（`file.write_all(...)`） |
| 重要度 | 低 |
| 分類 | 正確性 |
| 状態 | 対応済み |

**内容**

`create_new(true)` で `skill-lock.toml` を作成してから `write_all` で中身を書いている。`write_all` が失敗する（ディスク容量不足・クォータ超過など）とエラーは返るが、空または途中までのファイルがディスクに残る。

**影響**

再度 `skill-lock init` を実行すると `skill-lock.toml already exists` で失敗するため、ユーザーが手動でファイルを削除しない限り復旧できない。

**修正方針**

書き込みに失敗した場合は、作成したファイルを削除してからエラーを返す。

- 削除自体が失敗しても、元の書き込みエラーを優先して返す。
- 一時ファイル + rename 方式は、rename が既存ファイルを上書きしてしまい「既存なら上書きしない」要件と両立させにくいため採用しない。

**対応内容**

- ファイルの作成と書き込みを `create_new_with` に切り出し、書き込み失敗時に `fs::remove_file` で作成したファイルを削除するようにした。
- 書き込みを失敗させるテスト `removes_file_if_write_fails` を追加し、ファイルが残らないこと・その後 `init` を再実行できることを確認した。

## 問題なしと判断した点

- 雛形の内容が `docs/config.md` の仕様（`version = 1`、`settings.targets = ["claude"]`、コメントアウトした skill 記入例）と一致している。
- `create_new` により、存在確認と作成が 1 回の操作で行われ、TOCTOU の競合がない。
- エラーの変換、`main` での失敗時の終了コード、ユニットテスト 2 件（作成・既存時エラー）の範囲が適切。
