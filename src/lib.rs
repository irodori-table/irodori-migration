//! Database migration planning, schema diff, verification SQL, and tabular IO.
//!
//! This crate is intentionally execution-free by default: it builds plans,
//! SQL, scripts, manifests, and import/export streams, while host
//! applications own credentials, network access, scheduling, and approval UX.

#![forbid(unsafe_code)]

pub mod canonical;
pub mod checksum;
pub mod dialect;
pub mod export;
pub mod io;
pub mod plan;
pub mod recipe;
pub mod rollout;
pub mod schema;
mod sql_ref;

pub use canonical::{
    canonical_cell_sql, canonical_row_sql, canonical_value_sql, canonicalization_warnings,
    CanonicalColumn, CanonicalType, CanonicalizationPolicy, TimestampMode,
};
pub use checksum::{
    build_sync_repair_plan, checksum_diff_sql, checksum_manifest_table_sql,
    chunk_checksum_select_sql, pt_table_checksum_model, AdaptiveChunking, ChecksumAggregate,
    ChecksumFunction, ChunkBounds, ChunkChecksumConfig, SyncAlgorithm, SyncRepairPlan,
};
pub use export::{export_rows, ExportConfig, ExportControl, ExportReport};
pub use io::{
    generate_inserts_from_csv, infer_csv_schema, preview_delimited, preview_json, preview_ndjson,
    Cell, ColumnMapping, DelimitedEncoder, DelimitedImportOptions, DelimitedOptions, ImportColumn,
    ImportPreview, ImportPreviewOptions, InferredColumn, InferredType, JsonEncoder, NdjsonEncoder,
    OwnedCell, QuoteStyle, SqlColumnSpec, SqlInsertEncoder, SqlScriptEncoder, SqlScriptOptions,
    SqlWriteMode, TabularEncoder, UpsertStyle,
};
pub use plan::{
    build_migration_plan, build_migration_snippets, failed_bucket_row_diff_sql, fingerprint_sql,
    foreign_key_integrity_sql, hash_bucket_diff_sql, hash_bucket_fingerprint_sql,
    key_hash_expression, key_hash_profile_sql, keyed_diff_sql, manifest_table_sql,
    parse_column_list, partition_fingerprint_sql, row_hash_expression, row_hash_select_sql,
    vscode_snippet_body, ForeignKeySpec, MigrationEngine, MigrationExportFormat, MigrationPlan,
    MigrationSnippet, MigrationSnippetKind, MigrationSnippetVariable, MigrationSpec, MigrationTask,
    MigrationTaskLevel,
};
pub use recipe::{
    dry_run_text_recipe, recipe_run_summary, MigrationRecipe, RecipePhase, RecipePreview,
};
pub use rollout::{
    expand_contract_rollout, shadow_read_runbook, RolloutGate, RolloutPhase, RolloutPlan,
    RolloutStep, ShadowReadExperiment,
};
pub use schema::{
    diff_schemas, AlterColumnStyle, AlteredColumn, AlteredTable, Column, ColumnChange, Index,
    MigrationScript, MigrationStatement, Schema, SchemaDiff, Table,
};

#[cfg(feature = "avro")]
pub use io::AvroEncoder;

#[cfg(feature = "parquet")]
pub use io::ParquetEncoder;
