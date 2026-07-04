# Known Issues And Review Backlog

This file tracks the review findings that affect correctness, API shape,
feature completeness, tests, and application integration. Keep entries here
until the behavior is fixed and covered by tests.

## Correctness

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #1 | Fixed | row hash | Cross-engine row hashes used different digest algorithms. Postgres emitted MD5 while MySQL/Snowflake emitted SHA2-256 and Oracle used STANDARD_HASH, so identical Postgres -> MySQL rows could all appear changed. Row hashes now use MD5-compatible SQL per engine. |
| #2 | Fixed | row hash | Row-hash input used delimiter-only concatenation, which is not injective. Plan SQL now uses the canonical length-prefixed cell encoding. |
| #3 | Fixed | SQL identifiers | Identifier quoting was duplicated and `plan` had an escape hatch for names containing parentheses or pre-quotes. Plan, checksum, and canonical SQL now share one dialect-backed identifier renderer. |
| #4 | Fixed | checksum | Checksum SQL is now engine-aware. Unsupported function/aggregate combinations fail closed with `IRODORI_UNSUPPORTED_CHECKSUM_SQL(...)`, and MySQL MD5 integer conversion uses signed 64-bit semantics to match Postgres. |
| #5 | Fixed | canonical | Postgres float rounding casts through `NUMERIC`, boolean canonicalization preserves NULL, MySQL no longer emits silent-NULL `CONVERT_TZ`, and unsafe UTC timestamp modes fail closed for lakehouse/Trino-style engines. |
| #6 | Fixed | schema | Primary-key changes are diffed and rendered as drop/add primary key statements with destructive labeling for PK drops/changes. |
| #7 | Fixed | schema | Destructive labeling now covers type narrowing, nullability tightening, risky NOT NULL column additions, PK drops/changes, and constraint drops while index drops are not treated as data-destructive. |
| #8 | Fixed | schema | Incompatible `AlterColumnStyle`/dialect pairings now return typed unsupported migration entries instead of emitting Postgres-shaped SQL for other dialects. |
| #9 | Fixed | SQL import | SQL string output now escapes backslashes for MySQL-style dialects and delimited export guards spreadsheet-formula-looking text fields. |
| #10 | Fixed | encoders | Encoders and import inference reject non-finite floats instead of silently converting them to `0`, `NULL`, or invalid SQL literals. |
| #11 | Fixed | import inference | CSV/JSON import inference preserves leading-zero numeric text, keeps oversized unsigned JSON integers as text, strips UTF-8 BOM from headers, and shares safer CSV-to-SQL literal handling. |
| #12 | Fixed | Avro | `AvroEncoder` leaked schemas with `Box::leak` and used `Record::new(...).unwrap()`. It now owns the schema, writes through a short-lived Avro writer in `finish`, and returns `io::Error` for invalid schema/field names. |
| #13 | Fixed | Parquet/preview | Parquet export now flushes bounded row groups instead of retaining all rows, and import previews have separate display-row, scan-row, and JSON byte caps so preview paths no longer walk unbounded inputs by default. |

## Design And API

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #14 | Fixed | dialect API | `MigrationEngine` exposes a shared `SqlDialect` accessor, plan/checksum/canonical table rendering uses the shared path, and hot-path engine matches are exhaustive instead of catch-all defaults. |
| #15 | Fixed | table refs | `TableRef` models catalog/schema/name parts, renders through `SqlDialect::quote_qualified_identifier_if_needed`, and fails closed for empty, empty-part, or over-qualified dotted inputs. |
| #16 | Fixed | errors | `try_build_migration_plan` returns typed `MigrationPlanError` issues for invalid table refs, missing key columns, empty column names, and empty hash inputs; empty row-hash expressions fail closed with `IRODORI_UNSUPPORTED_SQL(...)`. |
| #17 | Fixed | public API | `MigrationSpec`/`ForeignKeySpec` have builder constructors, `AdaptiveChunking` now computes bounded next chunk sizes and throttle conditions, and sync repair output is typed as named-parameter templates instead of angle-bracket pseudo-SQL. |
| #18 | Fixed | schema model | Schema diffs now model foreign keys, CHECK constraints, UNIQUE constraints, primary key changes, and table/column/index/constraint rename hints. |
| #19 | Fixed | encoders | Encoders now validate row width, reject non-finite floats consistently, normalize SQL boolean casing, and expose fallible constructor/write paths where validation is needed. |

## Feature Gaps

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #20 | Fixed | chunking | `chunk_iteration_sql` now emits keyset-style chunk enumeration SQL and migration plans include it with the normalized `batch_size`. |
| #21 | Fixed | resume | Checkpoint table, chunk manifest table, resume query, and mark-completed SQL builders are covered by tests and included in generated target SQL. |
| #22 | Fixed | FK ordering | `foreign_key_load_order` computes parent-before-child ordering and defers cycles, self-references, and missing tables with focused coverage. |
| #23 | Fixed | DDL | Cross-engine type mapping and `target_table_ddl_sql` generate target DDL with lossy-conversion warnings and tests. |
| #24 | Fixed | rollout | `MigrationValidationSummary`, `validation_rollout_gates`, and `attach_validation_gates` connect rollout decisions to checksum, row-count, key-count, diff, and shadow-read evidence. |

## Tests And Integration

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #25 | Fixed | tests | Docker-gated tests now cover Postgres/MySQL checksum SQL, cross-engine row-hash equivalence, generated Postgres DDL/INSERT execution, and MySQL insert escaping. CI should run ignored container tests with `--ignored --test-threads=1`. |
| #26 | Fixed | tests | Added focused coverage for TableRef, typed plan errors, public import mapping/options, QuoteStyle::Always, SQL insert/upsert edge cases, empty JSON output, Parquet row-group flushing, checksum/canonical fail-closed paths, schema PK/constraint/rename diffs, and IO precision/backslash/BOM/row-width cases. |
| #27 | Open | desktop integration | `irodori-table` does not depend on this crate yet; the desktop migration studio has an incompatible TS planner. Prefer making this crate the Tauri command source of truth after row-hash negotiation is stable. |
| #28 | Fixed | miscellany | Hot-path null marker cloning was removed, line endings are validated, SQL Server `ORDER BY` detection ignores string/comment text, export cancellation is checked before each row, JSON oversized integers stay text, and empty JSON exports render cleanly. |
