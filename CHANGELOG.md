# Changelog

All notable changes to `irodori-migration` are documented here.

## 0.4.2 - 2026-07-23

- Added RustSec dependency auditing to continuous integration and releases.
- Added an automated, tag-validated GitHub release workflow with packaged crate
  artifacts and optional crates.io publication.
- Updated GitHub Actions and corrected stale license wording in the README.

## 0.4.1 - 2026-07-04

- Marked dropped indexes as destructive changes in schema diffs and generated
  migration statements.

## 0.4.0 - 2026-07-04

- Added `TableRef` and shared table-reference rendering for catalog/schema/table
  identifiers, including fail-closed parsing for ambiguous dotted references.
- Added `try_build_migration_plan` with typed `MigrationPlanError` issues for
  missing/invalid table refs, empty key columns, and empty hash inputs.
- Removed remaining hot-path `MigrationEngine` catch-all match arms from
  canonical, checksum, and DDL rendering helpers.
- Bounded import previews with separate display-row, scan-row, and JSON byte
  caps; NDJSON and delimited previews stop scanning after the configured limit.
- Changed Parquet export to flush bounded row groups instead of buffering all
  rows until `finish`.
- Implemented `AdaptiveChunking` sizing helpers and converted sync repair output
  to typed named-parameter templates while keeping a compatibility statement
  projection.
- Reduced export cancellation latency by checking cancellation before every row,
  fixed hot-path null marker cloning, improved SQL Server `ORDER BY` detection,
  and emitted clean empty JSON arrays.
- Expanded unit and Docker-gated tests for typed plan errors, TableRef quoting,
  preview bounds, IO edge cases, Parquet row-group flushing, and generated
  DDL/INSERT SQL execution.

## 0.3.0 - 2026-07-04

- Fixed cross-engine checksum generation to avoid MySQL-only SQL on unsupported
  engines and to keep Postgres/MySQL MD5 integer semantics aligned.
- Hardened canonical SQL for Postgres floats, boolean NULL handling, unsafe UTC
  timestamp normalization, and unsupported engine paths.
- Extended schema diffs with primary-key changes, FK/CHECK/UNIQUE constraints,
  rename hints, unsupported DDL entries, and sharper destructive-change labels.
- Hardened IO/import/export paths for MySQL backslash escaping, non-finite
  floats, row-width validation, UTF-8 BOM headers, oversized integers, leading
  zeroes, and spreadsheet-formula-looking delimited text.
- Added real chunk-iteration SQL to generated migration plans and refreshed SQL
  snapshots.
- Added resumable migration checkpoint SQL, FK-aware load ordering,
  cross-engine target DDL/type mapping helpers, and rollout gates backed by
  checksum/diff validation summaries.
- Added a shared `MigrationEngine::dialect()` bridge so generated SQL quoting
  uses one engine-to-dialect mapping.
- Expanded tests for checksum/canonical fail-closed behavior, schema DDL
  rendering, and IO edge cases.

## 0.2.0 - 2026-06-30

- Added tracing instrumentation for export and migration plan generation paths.
- Added snapshot coverage for generated migration SQL.
- Added container-managed Postgres/MySQL checksum SQL smoke tests.
- Documented container-managed and externally managed live SQL test flows.

## 0.1.3 - 2026-06-29

- Added ignored live SQL smoke tests for generated Postgres and MySQL checksum
  queries.
- Added CI services for Postgres/MySQL live SQL verification.
- Added an MSRV check job for Rust 1.88.
- Added `docs/testing.md`, `scripts/verify.sh`, and `SECURITY.md`.
- Included integration tests and scripts in packaged release artifacts.

## 0.1.2 - 2026-06-29

- Added CI workflow for formatting, tests, clippy, and package verification.
- Added development documentation, changelog, examples, and rustfmt config.
- Fixed crate metadata, README release instructions, gitignore coverage, and
  license naming.
- Marked the crate as `unsafe_code` forbidden.

## 0.1.1 - 2026-06-29

- Added cross-engine canonicalization policies for checksums.
- Added chunked checksum SQL, checksum manifests, divergent-chunk queries, and
  sync repair-plan scaffolding.
- Added recipe-style dry-run previews for generated artifacts.
- Added expand/contract rollout and shadow-read runbook helpers.

## 0.1.0 - 2026-06-29

- Initial standalone migration core.
- Added schema diff and destructive-change tagging.
- Added migration plans, row-hash SQL, bucket-level diff SQL, and failed-bucket
  row diff SQL.
- Added tabular import previews and CSV, TSV, SQL, JSON, NDJSON, Avro, and
  Parquet encoders.
