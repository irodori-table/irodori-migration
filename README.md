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

## License

0BSD. You can use, copy, modify, and distribute this project for almost any purpose.
