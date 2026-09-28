# サブコマンド仕様

- `init`: `skill-lock.toml`の雛形を作成する。既に存在する場合はエラーで終了する
- `lock`: `skill-lock.toml`から定義されているskillを`skill-lock.lock`にハッシュをロックする
- `install`: `skill-lock.lock`からハッシュに一致するskillをインストールする

## `lock`

`skill-lock.toml` の各 skill の ref をコミット SHA に解決し、skill ディレクトリの内容ハッシュとあわせて `skill-lock.lock` に書き出す。skill の配置は行わない。

- 既存の `skill-lock.lock` のエントリのうち、`github` / `path` / `requested` がマニフェストと一致するものは `commit` / `hash` をそのまま維持し、取得も行わない。`targets` のみマニフェストの値で更新する。
- 一致しないエントリ、および lock にない skill のみ取得して再解決する。
- マニフェストから削除された skill は lock から除く。
- 内容が変わらない場合は lock ファイルを書き換えない。
- 取得は git CLI で行う（[認証](config.md#認証) を参照）。実行環境に `git` が必要。
