# irodori-migration

Execution-free migration planning and data-diff primitives for Rust apps.

It generates SQL, plans, manifests, previews, and export streams. It never opens
database connections or stores credentials.

[crates.io](https://crates.io/crates/irodori-migration) |
[docs.rs](https://docs.rs/irodori-migration)

## Provides

- schema snapshots and diffs
- destructive-change labels
- migration runbooks
- row-hash and checksum SQL
- bucket and row-level diff SQL
- chunk iteration and checkpoint/resume SQL
- FK-aware load ordering
- cross-engine target DDL/type mapping helpers
- rollout gates backed by validation summaries
- CSV, TSV, SQL, JSON, NDJSON, Avro, and Parquet export helpers
- progress and cancellation hooks for host job systems

## Use

```toml
[dependencies]
irodori-migration = "0.3"
```

```rust
use irodori_migration::{build_migration_plan, MigrationEngine, MigrationSpec};

let spec = MigrationSpec {
    source_engine: MigrationEngine::Postgres,
    target_engine: MigrationEngine::MySql,
    source_table: "public.orders".into(),
    target_table: "orders".into(),
    key_columns: vec!["id".into()],
    compare_columns: vec!["id".into(), "amount".into()],
    ..MigrationSpec::default()
};

let plan = build_migration_plan(&spec);
println!("{}", plan.diff_sql);
```

## Develop

```sh
cargo fmt -- --check
cargo test
cargo test --all-features
cargo clippy --all-features --all-targets -- -D warnings
```

Live SQL smoke tests are ignored by default. See [docs/testing.md](docs/testing.md).

Review backlog and known correctness gaps are tracked in
[docs/known-issues.md](docs/known-issues.md).

License: `MIT OR 0BSD`.
