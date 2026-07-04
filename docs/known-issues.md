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
| #4 | Open | checksum | `checksum.rs` still allows MySQL-specific aggregate/function shapes such as `BIT_XOR`, `CONV`, and `CRC32` for engines where they are not portable. Postgres signed integer conversion and MySQL unsigned conversion can also disagree. |
| #5 | Open | canonical | Engine-specific canonicalization needs hardening: Postgres float rounding, boolean NULL handling, MySQL `CONVERT_TZ` returning NULL when timezone tables are missing, and DuckDB/Trino timestamp normalization. |
| #6 | Open | schema | Schema diff does not detect primary-key changes. |
| #7 | Open | schema | Destructive-change labeling is incomplete for both directions of some type/nullability/default changes. |
| #8 | Open | schema | `AlterColumnStyle::Standard` is effectively Postgres-oriented and should not be presented as fully generic ANSI DDL. |
| #9 | Open | SQL import | MySQL SQL string output does not escape backslashes. |
| #10 | Open | encoders | JSON/NDJSON non-finite floats are silently converted to `0`; SQL scripts convert them to `NULL`. |
| #11 | Open | import inference | CSV/JSON numeric inference can lose leading zeroes or precision for integers beyond `i64`. |
| #12 | Fixed | Avro | `AvroEncoder` leaked schemas with `Box::leak` and used `Record::new(...).unwrap()`. It now owns the schema, writes through a short-lived Avro writer in `finish`, and returns `io::Error` for invalid schema/field names. |
| #13 | Open | Parquet/preview | Parquet export and some preview paths still buffer all rows in memory. |

## Design And API

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #14 | Open | dialect API | `MigrationEngine` and `SqlDialect` are overlapping abstractions. Consolidate SQL rendering through one dialect interface and remove `_` catch-all SQL branches so unsupported engines fail closed. |
| #15 | Open | table refs | Table references are strings. Introduce a structured `TableRef` to distinguish catalog/schema/table parts from raw SQL expressions. |
| #16 | Open | errors | Placeholder SQL strings should become typed errors for missing keys, compare columns, or unsupported engine features. |
| #17 | Open | public API | Review unused or under-specified public APIs such as `AdaptiveChunking`. |
| #18 | Open | schema model | Add FK, CHECK, and rename modeling to schema diffs. |
| #19 | Open | encoders | Make encoder APIs consistent on streaming, finish semantics, and error behavior. |

## Feature Gaps

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #20 | Open | chunking | `batch_size` is described in runbooks but no generated SQL walks chunks. |
| #21 | Open | resume | Add checkpoint/resume support for migration validation. |
| #22 | Open | FK ordering | Add FK-aware topological ordering for load/validate phases. |
| #23 | Open | DDL | Add cross-engine type mapping and target DDL generation. |
| #24 | Open | rollout | Connect rollout gates to checksum/hash validation results. |

## Tests And Integration

| ID | Status | Area | Finding |
| --- | --- | --- | --- |
| #25 | Partially fixed | tests | Added a Docker-gated Postgres/MySQL row-hash equivalence test. CI should run ignored container tests with `--ignored --test-threads=1`. |
| #26 | Open | tests | Container coverage still focuses on generated SQL shape and should broaden to checksum/canonical equivalence, schema DDL smoke tests, and IO edge cases. |
| #27 | Open | desktop integration | `irodori-table` does not depend on this crate yet; the desktop migration studio has an incompatible TS planner. Prefer making this crate the Tauri command source of truth after row-hash negotiation is stable. |
| #28 | Open | miscellany | Remaining small review notes should be split into focused issues as adjacent code is touched. |

