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
| #13 | Partially fixed | Avro/Parquet/preview | NDJSON preview no longer buffers all parsed rows, and Parquet export now has a bounded row cap. Avro and JSON document preview still buffer by design and need a broader streaming API. |

## Design And API

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #14 | Partially fixed | dialect API | `MigrationEngine` now exposes a shared `SqlDialect` accessor and row-hash digest rendering avoids a catch-all arm. A fuller consolidation of all plan/canonical/checksum SQL rendering remains open. |
| #15 | Open | table refs | Table references are strings. Introduce a structured `TableRef` to distinguish catalog/schema/table parts from raw SQL expressions. |
| #16 | Open | errors | Placeholder SQL strings should become typed errors for missing keys, compare columns, or unsupported engine features. |
| #17 | Open | public API | Review unused or under-specified public APIs such as `AdaptiveChunking`. |
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
| #25 | Partially fixed | tests | Added a Docker-gated Postgres/MySQL row-hash equivalence test. CI should run ignored container tests with `--ignored --test-threads=1`. |
| #26 | Partially fixed | tests | Added focused coverage for checksum/canonical fail-closed paths, schema PK/constraint/rename diffs, and IO non-finite/precision/backslash/BOM/row-width edge cases. Broader container DDL/IO execution remains open. |
| #27 | Open | desktop integration | `irodori-table` does not depend on this crate yet; the desktop migration studio has an incompatible TS planner. Prefer making this crate the Tauri command source of truth after row-hash negotiation is stable. |
| #28 | Partially fixed | miscellany | Addressed adjacent IO cleanups such as line-ending validation, formula guarding, and JSON integer precision. Hot-path null cloning, dialect ORDER BY scanning, and export cancellation ergonomics remain open. |
