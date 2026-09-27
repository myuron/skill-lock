# 設定ファイル仕様

skill-lock は 2 つのファイルでプロジェクトの skill を管理する。

| ファイル | 作成者 | 役割 |
| --- | --- | --- |
| `skill-lock.toml` | ユーザー（手書き） | 導入したい skill とその取得元・配置先を宣言するマニフェスト |
| `skill-lock.lock` | skill-lock（自動生成） | 各 skill を解決した結果（commit SHA・内容ハッシュ）を記録するロックファイル |

- どちらもプロジェクトルートに置き、VCS にコミットする。
- `skill-lock.toml` は「何が欲しいか」、`skill-lock.lock` は「実際に何を入れたか」を表す（Cargo.toml / Cargo.lock と同じ関係）。
- lock ファイルがある限り、別の環境・別の時点で実行しても同じ内容の skill が配置される。

## skill-lock.toml

### 全体例

```toml
version = 1

[settings]
targets = ["claude"]            # デフォルトの配置先（省略時 ["claude"]）

[targets.shared]                # ユーザー定義ターゲット
path = "tools/skills"

[skills.pdf]
github = "anthropics/skills"
path = "skills/pdf"             # リポジトリ内の skill ディレクトリ（省略時はリポジトリルート）
tag = "v1.2.0"                  # tag / branch / rev は排他。省略時はデフォルトブランチ

[skills.my-review]
github = "myuron/my-skills"
path = "review"
branch = "main"
targets = ["claude", "codex"]   # skill 単位で配置先を上書き
```

### トップレベル

| キー | 型 | 必須 | 説明 |
| --- | --- | --- | --- |
| `version` | 整数 | ✓ | 設定ファイルのスキーマバージョン。現在は `1` のみ |

### `[settings]`

プロジェクト全体の既定値。テーブルごと省略できる。

| キー | 型 | 必須 | 既定値 | 説明 |
| --- | --- | --- | --- | --- |
| `targets` | 文字列の配列 | | `["claude"]` | skill 側で `targets` を指定しなかったときの配置先ターゲット名 |

### `[targets.<name>]`

配置先ターゲットの定義。`<name>` は `settings.targets` や `skills.<name>.targets` から参照する名前。

| キー | 型 | 必須 | 説明 |
| --- | --- | --- | --- |
| `path` | 文字列 | ✓ | skill を配置するディレクトリ。プロジェクトルートからの相対パス。skill は `<path>/<skill名>/` に置かれる |

#### 組み込みターゲット

定義しなくても使えるターゲット。同名の `[targets.<name>]` を書くと `path` を上書きできる。

| 名前 | 配置先 | 対象 |
| --- | --- | --- |
| `claude` | `.claude/skills` | Claude Code |
| `codex` | `.agents/skills` | Codex CLI |

> [!NOTE]
> 各エージェントの skill 読み込みパスは変わりうるため、実装時に最新の仕様を確認して組み込みターゲットを更新する。

#### パスの制約

- 絶対パス、およびプロジェクトルートの外を指すパス（`..` を含むなど）はエラー。
- 複数のターゲットが同じ `path` に解決される場合、同じ skill は 1 回だけ配置される。

### `[skills.<name>]`

導入する skill の宣言。1 テーブルが 1 skill に対応する。

`<name>` は skill 名であり、配置先ディレクトリ名になる（`.claude/skills/<name>/`）。`[a-z0-9-]+` のみ許可する。

| キー | 型 | 必須 | 説明 |
| --- | --- | --- | --- |
| `github` | 文字列 | ✓ | 取得元 GitHub リポジトリ。`owner/repo` 形式 |
| `path` | 文字列 | | リポジトリ内の skill ディレクトリ。省略時はリポジトリルート。このディレクトリ直下に `SKILL.md` が必要 |
| `tag` | 文字列 | | 取得するタグ |
| `branch` | 文字列 | | 取得するブランチ |
| `rev` | 文字列 | | 取得するコミット SHA（短縮形可） |
| `targets` | 文字列の配列 | | 配置先ターゲット名。指定すると `settings.targets` を**置き換える**（マージしない） |

- `tag` / `branch` / `rev` は同時に 1 つまで指定できる。いずれも省略した場合はリポジトリのデフォルトブランチを使う。
- `path` で指定したディレクトリの中身がそのまま配置先にコピーされる。

#### 取得元の拡張方針

現在サポートする取得元は `github` のみ。将来 `git`（任意の Git URL）や `local`（ローカルパス）などを追加する場合は、`github` と並ぶ**ソースキー**として追加する。1 つの skill に指定できるソースキーは常に 1 つだけとする。

```toml
# 将来の例（現時点では未サポート）
[skills.internal]
git = "https://gitlab.example.com/team/skills.git"
path = "internal"
```

## skill-lock.lock

`install` / `update` 実行時に skill-lock が生成・更新する。手で編集しない。

### 例

```toml
# このファイルは skill-lock が自動生成します。手で編集しないでください。
version = 1

[[skill]]
name = "my-review"
github = "myuron/my-skills"
path = "review"
requested = { branch = "main" }
commit = "9c1e4b7a2f3d8e6c5b4a39281706f5e4d3c2b1a0"
hash = "sha256-2f8a7c1e0b9d4f6a3c5e8b7d1a2f4c6e9b0d3a5c7e1f2b4d6a8c0e3f5b7d9a1c"
targets = ["claude", "codex"]

[[skill]]
name = "pdf"
github = "anthropics/skills"
path = "skills/pdf"
requested = { tag = "v1.2.0" }
commit = "3f2a9d8c7b6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a"
hash = "sha256-7b3e9a1c5d2f8e4b6a0c3d7f1e5b9a2c4d8f6e0b3a7c1d5e9f2b4a6c8d0e3f7b"
targets = ["claude"]
```

### フィールド

| キー | 説明 |
| --- | --- |
| `version` | lock ファイルのスキーマバージョン |
| `skill[].name` | skill 名（マニフェストの `<name>`） |
| `skill[].github` / `path` | マニフェストの値をそのまま記録 |
| `skill[].requested` | マニフェストで指定された ref（`{ tag = ... }` / `{ branch = ... }` / `{ rev = ... }`。省略時は `{}`） |
| `skill[].commit` | 解決された 40 桁のコミット SHA |
| `skill[].hash` | 配置した skill ディレクトリ内容のハッシュ |
| `skill[].targets` | 実際に配置したターゲット名 |

- `commit` により取得元を固定し、`hash` により取得内容の同一性と配置後の手動変更を検出する。
- `[[skill]]` は `name` の昇順で出力し、差分を安定させる。

### hash の算出方法

決定的であることを最優先にする。

1. skill ディレクトリ内の全ファイルを、`/` 区切りの相対パスでバイト順にソートする。
2. 各ファイルについて `相対パス`、`実行ビットの有無`、`内容の長さ`、`内容` を順に連結する。
3. 全体の SHA-256 を計算し、`sha256-<16進数>` の形式で記録する。

タイムスタンプやパーミッション（実行ビット以外）は含めない。シンボリックリンクはエラーとする。

## 動作の意味論

設定ファイルが各コマンドでどう解釈されるかを定める。

### `install`

1. マニフェストと lock を読み込む。
2. 各 skill について、lock のエントリの `github` / `path` / `requested` がマニフェストと一致すれば、lock の `commit` を使う。
3. 一致しない、または lock にない skill のみ ref を再解決し、lock を更新する。
4. 取得した内容の `hash` が lock と一致しない場合はエラー（同じ commit なのに内容が異なる＝異常）。
5. 各ターゲットへ配置する。`targets` の変更は再解決を伴わず、配置先の追加・削除のみ行う。

### `install --locked`

マニフェストと lock が一致しない場合（再解決が必要な skill がある場合）はエラーで終了し、lock を変更しない。CI での利用を想定する。

### `update [name...]`

指定した skill（省略時は全 skill）の ref を再解決し、最新の commit で lock を更新して配置する。`rev` 指定の skill は変化しない。

### 管理対象と削除

- skill-lock が触るのは lock に記録された `<ターゲットの path>/<name>/` のみ。
- マニフェストから削除された skill は、lock に記録された配置先から削除する。
- 配置先に lock で管理されていない同名ディレクトリが既に存在する場合は、上書きせずエラーとする。
- 配置済みディレクトリの内容が lock の `hash` と一致しない（手動で編集された）場合は、警告を出して上書きする。

### 認証

環境変数 `GITHUB_TOKEN` が設定されていれば GitHub API / ダウンロードに使用する。private リポジトリの取得や API レート制限の回避に用いる。トークンは設定ファイルには書かない。

## バリデーション

| 条件 | 扱い |
| --- | --- |
| `version` が未指定・未サポート | エラー |
| 未知のキー | エラー（タイポ検出のため） |
| `tag` / `branch` / `rev` を複数指定 | エラー |
| ソースキー（`github`）がない | エラー |
| `github` が `owner/repo` 形式でない | エラー |
| skill 名が `[a-z0-9-]+` に一致しない | エラー |
| 未定義のターゲット名を参照 | エラー |
| ターゲットの `path` が絶対パス・ルート外 | エラー |
| `path` のディレクトリに `SKILL.md` がない | エラー |
| `SKILL.md` の frontmatter の `name` が skill 名と異なる | 警告 |
