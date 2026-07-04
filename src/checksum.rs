//! Chunked checksum SQL builders inspired by pt-table-checksum and reladiff.

use crate::canonical::{canonical_row_sql, CanonicalColumn, CanonicalizationPolicy};
use crate::plan::MigrationEngine;
use crate::sql_ref::{column_ref, table_ref};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumFunction {
    Crc32,
    Fnv1a64,
    Fnv64,
    MurmurHash,
    Md5,
    Sha1,
}

impl ChecksumFunction {
    pub fn sql_name(self) -> &'static str {
        match self {
            Self::Crc32 => "CRC32",
            Self::Fnv1a64 => "FNV1A_64",
            Self::Fnv64 => "FNV_64",
            Self::MurmurHash => "MURMUR_HASH",
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumAggregate {
    BitXor,
    Sum,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkBounds {
    pub column: String,
    pub lower: Option<String>,
    pub upper: Option<String>,
    pub include_upper: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkChecksumConfig {
    pub table: String,
    pub columns: Vec<CanonicalColumn>,
    pub bounds: Option<ChunkBounds>,
    pub function: ChecksumFunction,
    pub aggregate: ChecksumAggregate,
    pub policy: CanonicalizationPolicy,
}

impl ChunkChecksumConfig {
    pub fn new(table: impl Into<String>, columns: Vec<CanonicalColumn>) -> Self {
        Self {
            table: table.into(),
            columns,
            bounds: None,
            function: ChecksumFunction::Md5,
            aggregate: ChecksumAggregate::Sum,
            policy: CanonicalizationPolicy::default(),
        }
    }

    pub fn with_bounds(mut self, bounds: ChunkBounds) -> Self {
        self.bounds = Some(bounds);
        self
    }

    pub fn with_function(mut self, function: ChecksumFunction) -> Self {
        self.function = function;
        self
    }

    pub fn with_aggregate(mut self, aggregate: ChecksumAggregate) -> Self {
        self.aggregate = aggregate;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveChunking {
    pub chunk_size: u64,
    pub chunk_time_seconds: f64,
    pub chunk_size_limit: f64,
    pub max_lag_seconds: Option<f64>,
    pub max_load: Vec<String>,
}

impl Default for AdaptiveChunking {
    fn default() -> Self {
        Self {
            chunk_size: 1_000,
            chunk_time_seconds: 0.5,
            chunk_size_limit: 2.0,
            max_lag_seconds: Some(1.0),
            max_load: vec!["Threads_running=25".to_string()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAlgorithm {
    Chunk,
    Nibble,
    GroupBy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRepairPlan {
    pub algorithm: SyncAlgorithm,
    pub statements: Vec<String>,
    pub notes: Vec<String>,
}

pub fn chunk_checksum_select_sql(engine: MigrationEngine, config: &ChunkChecksumConfig) -> String {
    let row = canonical_row_sql(engine, &config.columns, &config.policy);
    let checksum = match row_hash_as_integer(engine, config.function, &row)
        .and_then(|row_hash| aggregate_sql(engine, config.aggregate, config.function, &row_hash))
    {
        Ok(checksum) => checksum,
        Err(reason) => unsupported_checksum_sql(&reason),
    };
    let mut lines = vec![
        "-- Chunk checksum. Compare row_count and chunk_checksum on both sides before row diff."
            .to_string(),
        "SELECT".to_string(),
        "  COUNT(*) AS row_count,".to_string(),
        format!("  {checksum} AS chunk_checksum"),
        format!("FROM {}", table_ref(engine, &config.table)),
    ];
    if let Some(bounds) = &config.bounds {
        let predicate = chunk_predicate(engine, bounds);
        if !predicate.is_empty() {
            lines.push(format!("WHERE {predicate}"));
        }
    }
    lines.join("\n")
}

pub fn checksum_manifest_table_sql(engine: MigrationEngine, table: &str) -> String {
    let text = string_type(engine);
    let count = integer_type(engine);
    [
        "-- Stores per-chunk checksums. Source and target rows are compared by chunk_id.",
        &format!("CREATE TABLE {} (", table_ref(engine, table)),
        &format!("  chunk_id {text} NOT NULL,"),
        &format!("  lower_boundary {text},"),
        &format!("  upper_boundary {text},"),
        &format!("  source_cnt {count},"),
        &format!("  source_crc {text},"),
        &format!("  target_cnt {count},"),
        &format!("  target_crc {text},"),
        "  checked_at TIMESTAMP",
        ");",
    ]
    .join("\n")
}

pub fn checksum_diff_sql(engine: MigrationEngine, manifest_table: &str) -> String {
    [
        "-- Divergent chunks: recurse, row-diff, or repair only these ranges.",
        "SELECT chunk_id, lower_boundary, upper_boundary, source_cnt, target_cnt, source_crc, target_crc",
        &format!("FROM {}", table_ref(engine, manifest_table)),
        "WHERE source_cnt <> target_cnt",
        "   OR source_crc <> target_crc",
        "   OR (source_crc IS NULL AND target_crc IS NOT NULL)",
        "   OR (source_crc IS NOT NULL AND target_crc IS NULL)",
        "ORDER BY chunk_id",
    ]
    .join("\n")
}

pub fn pt_table_checksum_model() -> Vec<String> {
    vec![
        "Split each table into index-ordered chunks and adapt chunk size toward a target runtime.".to_string(),
        "Compute COUNT plus an order-independent aggregate of per-row hashes for every chunk.".to_string(),
        "Percona replication mode runs the identical statement on replicas through binary-log replication; cross-engine diff must run equivalent statements independently.".to_string(),
        "Throttle with chunk time, chunk-size-limit, max-load, and replica/source lag before continuing.".to_string(),
        "Only recurse into chunks whose count or checksum differs.".to_string(),
    ]
}

pub fn build_sync_repair_plan(
    engine: MigrationEngine,
    target_table: &str,
    key_columns: &[String],
    update_columns: &[String],
    algorithm: SyncAlgorithm,
) -> SyncRepairPlan {
    let keys = if key_columns.is_empty() {
        vec!["<primary_key>".to_string()]
    } else {
        key_columns.to_vec()
    };
    let updates = if update_columns.is_empty() {
        vec!["<changed_column>".to_string()]
    } else {
        update_columns.to_vec()
    };
    let target = table_ref(engine, target_table);
    let key_predicate = keys
        .iter()
        .map(|key| format!("{} = <source.{}>", column_ref(engine, key), key))
        .collect::<Vec<_>>()
        .join(" AND ");
    let update_list = updates
        .iter()
        .map(|column| format!("{} = <source.{}>", column_ref(engine, column), column))
        .collect::<Vec<_>>()
        .join(", ");
    let statements = vec![
        format!("-- Re-checksum sub-chunks before executing repair SQL against {target}."),
        format!("UPDATE {target} SET {update_list} WHERE {key_predicate};"),
        format!("INSERT INTO {target} (<columns>) VALUES (<source values>);"),
        format!("DELETE FROM {target} WHERE {key_predicate};"),
    ];
    let notes = match algorithm {
        SyncAlgorithm::Chunk => vec![
            "Chunk: checksum large ranges first, then inspect rows only inside failed chunks."
                .to_string(),
        ],
        SyncAlgorithm::Nibble => vec![
            "Nibble: walk key ranges in small ordered batches when the full chunk is too large."
                .to_string(),
        ],
        SyncAlgorithm::GroupBy => vec![
            "GroupBy: fallback for tables without a useful chunking key; expect higher cost."
                .to_string(),
        ],
    };
    SyncRepairPlan {
        algorithm,
        statements,
        notes,
    }
}

fn row_hash_as_integer(
    engine: MigrationEngine,
    function: ChecksumFunction,
    canonical_row: &str,
) -> Result<String, String> {
    match function {
        ChecksumFunction::Crc32
            if matches!(engine, MigrationEngine::MySql | MigrationEngine::MariaDb) =>
        {
            Ok(format!("CRC32({canonical_row})"))
        }
        ChecksumFunction::Crc32 => Err(format!(
            "{} checksum SQL does not support CRC32 row hashes",
            engine.label()
        )),
        ChecksumFunction::Fnv1a64 | ChecksumFunction::Fnv64 | ChecksumFunction::MurmurHash => {
            Err(format!(
                "{} checksum SQL does not support {} row hashes",
                engine.label(),
                function.sql_name()
            ))
        }
        ChecksumFunction::Md5 => md5_as_integer(engine, canonical_row),
        ChecksumFunction::Sha1 => sha1_as_integer(engine, canonical_row),
    }
}

fn md5_as_integer(engine: MigrationEngine, value: &str) -> Result<String, String> {
    hex_digest_as_integer(engine, &md5_hex_sql(engine, value), 17)
}

fn sha1_as_integer(engine: MigrationEngine, value: &str) -> Result<String, String> {
    hex_digest_as_integer(engine, &sha1_hex_sql(engine, value), 25)
}

fn md5_hex_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Oracle => format!("STANDARD_HASH({value}, 'MD5')"),
        MigrationEngine::TrinoPresto => format!("TO_HEX(MD5(TO_UTF8({value})))"),
        _ => format!("MD5({value})"),
    }
}

fn sha1_hex_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Oracle => format!("STANDARD_HASH({value}, 'SHA1')"),
        MigrationEngine::Postgres => format!("ENCODE(DIGEST({value}, 'sha1'), 'hex')"),
        MigrationEngine::TrinoPresto => format!("TO_HEX(SHA1(TO_UTF8({value})))"),
        _ => format!("SHA1({value})"),
    }
}

fn hex_digest_as_integer(
    engine: MigrationEngine,
    digest_hex: &str,
    start: usize,
) -> Result<String, String> {
    match engine {
        MigrationEngine::Postgres | MigrationEngine::Redshift => Ok(format!(
            "(('x' || SUBSTRING({digest_hex}, {start}))::bit(64)::bigint)"
        )),
        MigrationEngine::MySql | MigrationEngine::MariaDb => {
            let unsigned =
                format!("CAST(CONV(SUBSTRING({digest_hex}, {start}), 16, 10) AS DECIMAL(20,0))");
            Ok(format!(
                "(CASE WHEN {unsigned} >= 9223372036854775808 THEN {unsigned} - 18446744073709551616 ELSE {unsigned} END)"
            ))
        }
        MigrationEngine::Oracle => Ok(format!(
            "TO_NUMBER(SUBSTR({digest_hex}, {start}), 'XXXXXXXXXXXXXXXX')"
        )),
        MigrationEngine::Snowflake => Ok(format!(
            "TO_NUMBER(SUBSTR({digest_hex}, {start}), 'XXXXXXXXXXXXXXXX')"
        )),
        MigrationEngine::TrinoPresto => Ok(format!(
            "CAST(FROM_BASE(SUBSTR({digest_hex}, {start}), 16) AS DECIMAL(38,0))"
        )),
        MigrationEngine::Hive | MigrationEngine::Databricks => Ok(format!(
            "CAST(CONV(SUBSTR({digest_hex}, {start}), 16, 10) AS DECIMAL(38,0))"
        )),
        MigrationEngine::DuckDb | MigrationEngine::Iceberg | MigrationEngine::S3Tables => {
            Err(format!(
                "{} checksum SQL does not support portable hex digest to integer conversion",
                engine.label()
            ))
        }
    }
}

fn aggregate_sql(
    engine: MigrationEngine,
    aggregate: ChecksumAggregate,
    function: ChecksumFunction,
    row_hash: &str,
) -> Result<String, String> {
    match aggregate {
        ChecksumAggregate::BitXor
            if function == ChecksumFunction::Crc32
                && matches!(engine, MigrationEngine::MySql | MigrationEngine::MariaDb) =>
        {
            Ok(format!(
                "COALESCE(BIT_XOR(CAST({row_hash} AS UNSIGNED)), 0)"
            ))
        }
        ChecksumAggregate::BitXor => Err(format!(
            "{} checksum SQL does not support portable BIT_XOR aggregation for {}",
            engine.label(),
            function.sql_name()
        )),
        ChecksumAggregate::Sum => Ok(format!(
            "COALESCE(SUM({}), 0)",
            decimal_cast_sql(engine, row_hash)
        )),
    }
}

fn decimal_cast_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Oracle | MigrationEngine::Snowflake => {
            format!("CAST({value} AS NUMBER(38,0))")
        }
        MigrationEngine::Postgres | MigrationEngine::Redshift => {
            format!("CAST({value} AS NUMERIC(38,0))")
        }
        _ => format!("CAST({value} AS DECIMAL(38,0))"),
    }
}

fn unsupported_checksum_sql(reason: &str) -> String {
    format!("IRODORI_UNSUPPORTED_CHECKSUM_SQL({})", sql_string(reason))
}

fn sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn chunk_predicate(engine: MigrationEngine, bounds: &ChunkBounds) -> String {
    let column = column_ref(engine, &bounds.column);
    let mut parts = Vec::new();
    if let Some(lower) = &bounds.lower {
        parts.push(format!("{column} >= {lower}"));
    }
    if let Some(upper) = &bounds.upper {
        let op = if bounds.include_upper { "<=" } else { "<" };
        parts.push(format!("{column} {op} {upper}"));
    }
    parts.join(" AND ")
}

fn string_type(engine: MigrationEngine) -> &'static str {
    match engine {
        MigrationEngine::Hive | MigrationEngine::Databricks => "STRING",
        _ => "VARCHAR",
    }
}

fn integer_type(engine: MigrationEngine) -> &'static str {
    match engine {
        MigrationEngine::Oracle => "NUMBER",
        _ => "BIGINT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::CanonicalType;

    #[test]
    fn chunk_checksum_uses_count_and_order_independent_sum() {
        let config = ChunkChecksumConfig::new(
            "public.orders",
            vec![
                CanonicalColumn::new("id", CanonicalType::Integer),
                CanonicalColumn::new("amount", CanonicalType::Decimal { scale: 2 }),
            ],
        )
        .with_bounds(ChunkBounds {
            column: "id".to_string(),
            lower: Some("100".to_string()),
            upper: Some("200".to_string()),
            include_upper: false,
        });

        let sql = chunk_checksum_select_sql(MigrationEngine::Postgres, &config);

        assert!(sql.contains("COUNT(*) AS row_count"));
        assert!(sql.contains("SUM(CAST"));
        assert!(sql.contains("NUMERIC(38,0)"));
        assert!(sql.contains("MD5"));
        assert!(sql.contains("id >= 100 AND id < 200"));
        assert!(!sql.contains("UNSIGNED"));
    }

    #[test]
    fn percona_style_crc32_bitxor_is_expressible() {
        let config = ChunkChecksumConfig::new(
            "orders",
            vec![CanonicalColumn::new("id", CanonicalType::Integer)],
        )
        .with_function(ChecksumFunction::Crc32)
        .with_aggregate(ChecksumAggregate::BitXor);

        let sql = chunk_checksum_select_sql(MigrationEngine::MySql, &config);

        assert!(sql.contains("BIT_XOR"));
        assert!(sql.contains("CRC32"));
    }

    #[test]
    fn sha1_uses_sha1_digest_not_md5_of_sha1_text() {
        let config = ChunkChecksumConfig::new(
            "orders",
            vec![CanonicalColumn::new("id", CanonicalType::Integer)],
        )
        .with_function(ChecksumFunction::Sha1);

        let sql = chunk_checksum_select_sql(MigrationEngine::MySql, &config);

        assert!(sql.contains("SHA1"));
        assert!(sql.contains("SUBSTRING(SHA1"));
        assert!(!sql.contains("MD5(SHA1"));
    }

    #[test]
    fn unsupported_checksum_combinations_fail_closed() {
        let config = ChunkChecksumConfig::new(
            "public.orders",
            vec![CanonicalColumn::new("id", CanonicalType::Integer)],
        )
        .with_function(ChecksumFunction::Crc32)
        .with_aggregate(ChecksumAggregate::BitXor);

        let sql = chunk_checksum_select_sql(MigrationEngine::Postgres, &config);

        assert!(sql.contains("IRODORI_UNSUPPORTED_CHECKSUM_SQL"));
        assert!(!sql.contains("BIT_XOR(CAST"));
        assert!(!sql.contains("UNSIGNED"));
    }

    #[test]
    fn mysql_md5_sum_uses_signed_sixty_four_bit_semantics() {
        let config = ChunkChecksumConfig::new(
            "orders",
            vec![CanonicalColumn::new("id", CanonicalType::Integer)],
        )
        .with_function(ChecksumFunction::Md5)
        .with_aggregate(ChecksumAggregate::Sum);

        let sql = chunk_checksum_select_sql(MigrationEngine::MySql, &config);

        assert!(sql.contains("9223372036854775808"));
        assert!(sql.contains("18446744073709551616"));
        assert!(sql.contains("SUM(CAST"));
        assert!(sql.contains("DECIMAL(38,0)"));
    }

    #[test]
    fn checksum_manifest_tracks_source_and_target_crc() {
        let sql =
            checksum_manifest_table_sql(MigrationEngine::Snowflake, "irodori_chunk_checksums");

        assert!(sql.contains("source_crc"));
        assert!(sql.contains("target_crc"));
        assert!(sql.contains("lower_boundary"));
    }

    #[test]
    fn sync_repair_plan_mentions_update_insert_delete() {
        let plan = build_sync_repair_plan(
            MigrationEngine::Postgres,
            "public.orders",
            &["id".to_string()],
            &["amount".to_string()],
            SyncAlgorithm::Chunk,
        );

        assert!(plan.statements.iter().any(|sql| sql.starts_with("UPDATE")));
        assert!(plan.statements.iter().any(|sql| sql.starts_with("INSERT")));
        assert!(plan.statements.iter().any(|sql| sql.starts_with("DELETE")));
    }
}
