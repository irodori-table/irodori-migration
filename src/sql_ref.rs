use crate::dialect::{MySqlDialect, OracleDialect, PostgresDialect, SnowflakeDialect, SqlDialect};
use crate::plan::MigrationEngine;

pub(crate) fn table_ref(engine: MigrationEngine, name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "(missing_table)".to_string();
    }
    qualified_ref(engine, trimmed)
}

pub(crate) fn column_ref(engine: MigrationEngine, name: &str) -> String {
    qualified_ref(engine, name.trim())
}

pub(crate) fn identifier_ref(engine: MigrationEngine, value: &str) -> String {
    dialect_for_engine(engine).quote_identifier_if_needed(value.trim())
}

fn qualified_ref(engine: MigrationEngine, value: &str) -> String {
    value
        .split('.')
        .map(|part| identifier_ref(engine, part))
        .collect::<Vec<_>>()
        .join(".")
}

fn dialect_for_engine(engine: MigrationEngine) -> &'static dyn SqlDialect {
    match engine {
        MigrationEngine::MySql
        | MigrationEngine::MariaDb
        | MigrationEngine::Hive
        | MigrationEngine::Databricks => &MySqlDialect,
        MigrationEngine::Oracle => &OracleDialect,
        MigrationEngine::Snowflake => &SnowflakeDialect,
        MigrationEngine::Postgres
        | MigrationEngine::DuckDb
        | MigrationEngine::Iceberg
        | MigrationEngine::S3Tables
        | MigrationEngine::Redshift
        | MigrationEngine::TrinoPresto => &PostgresDialect,
    }
}
