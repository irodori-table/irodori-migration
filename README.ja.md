<!-- i18n: language-switcher -->
[English](README.md) | [日本語](README.ja.md)

# irodori-migration

Rustアプリ向けの実行不要なマイグレーション計画およびデータ差分プリミティブ。

SQLの生成、計画、マニフェスト、プレビュー、エクスポートストリームを提供します。
データベース接続を開いたり、認証情報を保存したりすることはありません。

[crates.io](https://crates.io/crates/irodori-migration) |
[docs.rs](https://docs.rs/irodori-migration)

## 提供機能

- スキーマスナップショットと差分
- 破壊的変更ラベル
- マイグレーション実行手順書
- 行ハッシュおよびチェックサムSQL
- バケットおよび行レベルの差分SQL
- チャンク反復およびチェックポイント／再開SQL
- 外部キーを考慮したロード順序
- クロスエンジン対応のターゲットDDL／型マッピングヘルパー
- 検証サマリーに基づくロールアウトゲート
- CSV、TSV、SQL、JSON、NDJSON、Avro、Parquetのエクスポートヘルパー
- ホストジョブシステム向けの進捗およびキャンセルフック

## 使い方

```toml
[dependencies]
irodori-migration = "0.4"
```

```rust
use irodori_migration::{try_build_migration_plan, MigrationEngine, MigrationSpec};

let spec = MigrationSpec::new(
    MigrationEngine::Postgres,
    MigrationEngine::MySql,
    "public.orders",
    "orders",
)
.with_key_columns(vec!["id".into()])
.with_compare_columns(vec!["id".into(), "amount".into()]);

let plan = try_build_migration_plan(&spec)?;
println!("{}", plan.diff_sql);
# Ok::<(), irodori_migration::MigrationPlanError>(())
```

## 開発

```sh
cargo fmt -- --check
cargo test
cargo test --all-features
cargo clippy --all-features --all-targets -- -D warnings
```

ライブSQLスモークテストはデフォルトで無視されます。詳細は[docs/testing.md](docs/testing.md)を参照してください。

レビューのバックログおよび既知の正確性のギャップは[docs/known-issues.md](docs/known-issues.md)で管理しています。

ライセンス: `MIT OR 0BSD`。

## ライセンス

0BSD。ほぼあらゆる目的でこのプロジェクトを使用、コピー、改変、配布できます。