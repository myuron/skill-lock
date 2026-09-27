# サブコマンド仕様

- `init`: `skill-lock.toml`の雛形を作成する。既に存在する場合はエラーで終了する
- `lock`: `skill-lock.toml`から定義されているskillを`skill-lock.lock`にハッシュをロックする
- `install`: `skill-lock.lock`からハッシュに一致するskillをインストールする
