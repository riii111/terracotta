# 開発時の検証

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --locked --release
```

## 描画snapshot

`cargo install cargo-insta --locked` と `cargo install cargo-nextest --locked` で検証ツールを用意する。
全testと未参照snapshotの検査はCIと同じコマンドで実行する。

```sh
cargo fetch --locked
NEXTEST_PROFILE=ci cargo insta test --test-runner nextest --disable-nextest-doctest --unreferenced=reject --all-targets --all-features
```

表示を意図して変更した場合は、候補を生成して差分を確認・承認する。

```sh
cargo insta test --test-runner nextest --disable-nextest-doctest --lib -- render_snapshots
cargo insta review
```

承認した `.snap` を変更とともにcommitし、全testの検査を再実行する。`.snap.new` はcommitしない。

snapshotは `src/ui/<画面>/tests/` に置く。固定サイズの `TestBackend` の各セルを文字列化し、行末空白を除去して比較する。実行画面は開始時刻からの経過時間を固定する。色・装飾は既存の明示assertionで検証する。
