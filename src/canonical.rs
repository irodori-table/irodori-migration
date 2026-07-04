//! Cross-engine value canonicalization for stable row hashing.
//!
//! The functions in this module generate SQL fragments only. They do not claim
//! that every engine can make every value byte-identical; instead they make the
//! normalization choices explicit so a host can preview them and reject risky
//! comparisons before running hashes.

use crate::plan::MigrationEngine;
use crate::sql_ref::column_ref;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampMode {
    Naive,
    Utc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalType {
    Text,
    Integer,
    Decimal {
        scale: u8,
    },
    Float {
        precision: u8,
    },
    Boolean,
    Timestamp {
        fractional_digits: u8,
        mode: TimestampMode,
    },
    Date,
    Uuid,
    Bytes,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalColumn {
    pub name: String,
    pub value_type: CanonicalType,
}

impl CanonicalColumn {
    pub fn new(name: impl Into<String>, value_type: CanonicalType) -> Self {
        Self {
            name: name.into(),
            value_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalizationPolicy {
    pub null_token: String,
    pub delimiter: String,
    pub trim_strings: bool,
    pub normalize_case: bool,
    pub empty_string_is_null: bool,
    pub length_prefix_values: bool,
}

impl Default for CanonicalizationPolicy {
    fn default() -> Self {
        Self {
            null_token: "__IRODORI_NULL__".to_string(),
            delimiter: "\u{1f}".to_string(),
            trim_strings: true,
            normalize_case: false,
            empty_string_is_null: false,
            length_prefix_values: true,
        }
    }
}

pub fn canonical_row_sql(
    engine: MigrationEngine,
    columns: &[CanonicalColumn],
    policy: &CanonicalizationPolicy,
) -> String {
    let cells = columns
        .iter()
        .map(|column| canonical_cell_sql(engine, column, policy))
        .collect::<Vec<_>>();
    concat_sql(engine, &cells, &policy.delimiter)
}

pub(crate) fn canonical_row_from_values_sql(
    engine: MigrationEngine,
    values: &[String],
    policy: &CanonicalizationPolicy,
) -> String {
    let cells = values
        .iter()
        .map(|value| canonical_cell_from_value_sql(engine, value, policy))
        .collect::<Vec<_>>();
    concat_sql(engine, &cells, &policy.delimiter)
}

pub fn canonical_cell_sql(
    engine: MigrationEngine,
    column: &CanonicalColumn,
    policy: &CanonicalizationPolicy,
) -> String {
    let raw = column_ref(engine, &column.name);
    let value = canonical_value_sql(engine, &raw, &column.value_type, policy);
    let value = if should_empty_string_be_null(engine, policy) {
        format!("NULLIF({value}, '')")
    } else {
        value
    };
    if policy.length_prefix_values {
        length_prefixed_cell_sql(engine, &value, &policy.null_token)
    } else {
        format!("COALESCE({value}, {})", sql_string(&policy.null_token))
    }
}

fn canonical_cell_from_value_sql(
    engine: MigrationEngine,
    value: &str,
    policy: &CanonicalizationPolicy,
) -> String {
    let value = if should_empty_string_be_null(engine, policy) {
        format!("NULLIF({value}, '')")
    } else {
        value.to_string()
    };
    if policy.length_prefix_values {
        length_prefixed_cell_sql(engine, &value, &policy.null_token)
    } else {
        format!("COALESCE({value}, {})", sql_string(&policy.null_token))
    }
}

fn length_prefixed_cell_sql(engine: MigrationEngine, value: &str, null_token: &str) -> String {
    let payload = concat_raw_sql(
        engine,
        &[length_sql(engine, value), sql_string(":"), value.to_string()],
    );
    format!(
        "CASE WHEN {value} IS NULL THEN {} ELSE {} END",
        sql_string(null_token),
        concat_raw_sql(engine, &[sql_string("V"), payload])
    )
}

pub fn canonical_value_sql(
    engine: MigrationEngine,
    value: &str,
    value_type: &CanonicalType,
    policy: &CanonicalizationPolicy,
) -> String {
    let mut rendered = match value_type {
        CanonicalType::Text => text_sql(engine, value),
        CanonicalType::Integer => text_sql(engine, value),
        CanonicalType::Decimal { scale } => decimal_sql(engine, value, (*scale).min(18)),
        CanonicalType::Float { precision } => float_sql(engine, value, (*precision).min(12)),
        CanonicalType::Boolean => boolean_sql(engine, value),
        CanonicalType::Timestamp {
            fractional_digits,
            mode,
        } => timestamp_sql(engine, value, (*fractional_digits).min(6), *mode),
        CanonicalType::Date => date_sql(engine, value),
        CanonicalType::Uuid => uuid_sql(engine, value),
        CanonicalType::Bytes => bytes_sql(engine, value),
        CanonicalType::Json => text_sql(engine, value),
    };

    if policy.trim_strings
        && matches!(
            value_type,
            CanonicalType::Text | CanonicalType::Uuid | CanonicalType::Json
        )
    {
        rendered = format!("TRIM({rendered})");
    }
    if policy.normalize_case && matches!(value_type, CanonicalType::Text | CanonicalType::Uuid) {
        rendered = format!("LOWER({rendered})");
    }
    rendered
}

pub fn canonicalization_warnings(
    engine: MigrationEngine,
    columns: &[CanonicalColumn],
    policy: &CanonicalizationPolicy,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if engine == MigrationEngine::Oracle && !policy.empty_string_is_null {
        warnings.push("Oracle treats empty strings as NULL; enable empty_string_is_null when comparing text columns against other engines.".to_string());
    }
    if columns
        .iter()
        .any(|column| matches!(column.value_type, CanonicalType::Float { .. }))
    {
        warnings.push("Floating point hashes are only stable when both sides agree on rounding precision and special values.".to_string());
    }
    if columns.iter().any(|column| {
        matches!(
            column.value_type,
            CanonicalType::Timestamp {
                mode: TimestampMode::Naive,
                ..
            }
        )
    }) {
        warnings.push("Naive timestamps must be compared only after source and target session time zones are pinned.".to_string());
    }
    if columns
        .iter()
        .any(|column| matches!(column.value_type, CanonicalType::Bytes))
    {
        warnings.push("Binary values are rendered as uppercase hex; verify byte order for UUID-like binary columns.".to_string());
    }
    warnings
}

fn text_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Postgres | MigrationEngine::Redshift => format!("CAST({value} AS TEXT)"),
        MigrationEngine::Oracle => format!("TO_CHAR({value})"),
        MigrationEngine::Snowflake => format!("TO_VARCHAR({value})"),
        MigrationEngine::MySql | MigrationEngine::MariaDb => format!("CAST({value} AS CHAR)"),
        MigrationEngine::DuckDb | MigrationEngine::Iceberg | MigrationEngine::S3Tables => {
            format!("CAST({value} AS VARCHAR)")
        }
        MigrationEngine::Hive | MigrationEngine::Databricks => format!("CAST({value} AS STRING)"),
        MigrationEngine::TrinoPresto => format!("CAST({value} AS VARCHAR)"),
    }
}

fn decimal_sql(engine: MigrationEngine, value: &str, scale: u8) -> String {
    match engine {
        MigrationEngine::Oracle => {
            let format = decimal_format(scale);
            format!("TO_CHAR({value}, '{format}')")
        }
        MigrationEngine::Snowflake => {
            let format = decimal_format(scale);
            format!("TO_CHAR(TO_DECIMAL({value}, 38, {scale}), '{format}')")
        }
        MigrationEngine::Hive | MigrationEngine::Databricks => {
            format!("CAST(CAST({value} AS DECIMAL(38,{scale})) AS STRING)")
        }
        MigrationEngine::MySql | MigrationEngine::MariaDb => {
            format!("CAST(CAST({value} AS DECIMAL(38,{scale})) AS CHAR)")
        }
        _ => text_sql(engine, &format!("CAST({value} AS DECIMAL(38,{scale}))")),
    }
}

fn float_sql(engine: MigrationEngine, value: &str, precision: u8) -> String {
    text_sql(engine, &format!("ROUND({value}, {precision})"))
}

fn boolean_sql(engine: MigrationEngine, value: &str) -> String {
    let expression = match engine {
        MigrationEngine::MySql | MigrationEngine::MariaDb => {
            format!("CASE WHEN {value} THEN 1 ELSE 0 END")
        }
        _ => format!("CASE WHEN {value} THEN 1 ELSE 0 END"),
    };
    text_sql(engine, &expression)
}

fn timestamp_sql(
    engine: MigrationEngine,
    value: &str,
    fractional_digits: u8,
    mode: TimestampMode,
) -> String {
    let value = if mode == TimestampMode::Utc {
        timestamp_utc_sql(engine, value)
    } else {
        value.to_string()
    };
    let precision = fractional_digits.min(6);
    match engine {
        MigrationEngine::Oracle => {
            let format = if precision == 0 {
                "YYYY-MM-DD HH24:MI:SS".to_string()
            } else {
                format!("YYYY-MM-DD HH24:MI:SS.FF{precision}")
            };
            let rendered = format!("TO_CHAR(CAST({value} AS TIMESTAMP({precision})), '{format}')");
            pad_fraction(engine, &rendered, precision)
        }
        MigrationEngine::Postgres | MigrationEngine::Redshift => {
            let rendered =
                format!("TO_CHAR(CAST({value} AS TIMESTAMP(6)), 'YYYY-MM-DD HH24:MI:SS.US')");
            pad_fraction(engine, &rendered, precision)
        }
        MigrationEngine::Snowflake => {
            let rendered = format!(
                "TO_VARCHAR(CAST({value} AS TIMESTAMP_NTZ(6)), 'YYYY-MM-DD HH24:MI:SS.FF6')"
            );
            pad_fraction(engine, &rendered, precision)
        }
        MigrationEngine::MySql | MigrationEngine::MariaDb => {
            let rendered =
                format!("DATE_FORMAT(CAST({value} AS DATETIME(6)), '%Y-%m-%d %H:%i:%s.%f')");
            pad_fraction(engine, &rendered, precision)
        }
        MigrationEngine::Hive | MigrationEngine::Databricks => {
            let rendered =
                format!("DATE_FORMAT(CAST({value} AS TIMESTAMP), 'yyyy-MM-dd HH:mm:ss.SSSSSS')");
            pad_fraction(engine, &rendered, precision)
        }
        _ => pad_fraction(engine, &text_sql(engine, &value), precision),
    }
}

fn date_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Oracle => format!("TO_CHAR({value}, 'YYYY-MM-DD')"),
        MigrationEngine::Snowflake => format!("TO_VARCHAR({value}, 'YYYY-MM-DD')"),
        MigrationEngine::MySql | MigrationEngine::MariaDb => {
            format!("DATE_FORMAT({value}, '%Y-%m-%d')")
        }
        _ => text_sql(engine, &format!("CAST({value} AS DATE)")),
    }
}

fn uuid_sql(engine: MigrationEngine, value: &str) -> String {
    format!("LOWER({})", text_sql(engine, value))
}

fn bytes_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Postgres | MigrationEngine::Redshift => {
            format!("UPPER(ENCODE({value}, 'hex'))")
        }
        MigrationEngine::Oracle => format!("UPPER(RAWTOHEX({value}))"),
        MigrationEngine::Snowflake => format!("UPPER(HEX_ENCODE({value}))"),
        MigrationEngine::MySql | MigrationEngine::MariaDb => format!("UPPER(HEX({value}))"),
        MigrationEngine::TrinoPresto => format!("UPPER(TO_HEX({value}))"),
        _ => format!("UPPER(HEX({value}))"),
    }
}

fn timestamp_utc_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Postgres | MigrationEngine::Redshift => {
            format!("({value} AT TIME ZONE 'UTC')")
        }
        MigrationEngine::Snowflake => format!("CONVERT_TIMEZONE('UTC', {value})"),
        MigrationEngine::MySql | MigrationEngine::MariaDb => {
            format!("CONVERT_TZ({value}, @@session.time_zone, '+00:00')")
        }
        MigrationEngine::Oracle => format!("SYS_EXTRACT_UTC({value})"),
        _ => value.to_string(),
    }
}

fn pad_fraction(engine: MigrationEngine, value: &str, precision: u8) -> String {
    if precision >= 6 {
        return value.to_string();
    }
    let keep = if precision == 0 {
        19
    } else {
        20 + usize::from(precision)
    };
    let width = if precision == 0 { 19 } else { 26 };
    let prefix = match engine {
        MigrationEngine::Oracle => format!("SUBSTR({value}, 1, {keep})"),
        _ => format!("LEFT({value}, {keep})"),
    };
    match engine {
        MigrationEngine::Oracle => format!("RPAD({prefix}, {width}, '0')"),
        _ => format!("RPAD({prefix}, {width}, '0')"),
    }
}

fn concat_sql(engine: MigrationEngine, values: &[String], delimiter: &str) -> String {
    if values.is_empty() {
        return "''".to_string();
    }
    let mut parts = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            parts.push(sql_string(delimiter));
        }
        parts.push(value.clone());
    }
    concat_raw_sql(engine, &parts)
}

fn concat_raw_sql(engine: MigrationEngine, values: &[String]) -> String {
    if values.is_empty() {
        return "''".to_string();
    }
    if engine == MigrationEngine::Oracle {
        values.join(" || ")
    } else {
        format!("CONCAT({})", values.join(", "))
    }
}

fn length_sql(engine: MigrationEngine, value: &str) -> String {
    match engine {
        MigrationEngine::Oracle => format!("LENGTH({value})"),
        _ => format!("LENGTH({value})"),
    }
}

fn should_empty_string_be_null(engine: MigrationEngine, policy: &CanonicalizationPolicy) -> bool {
    policy.empty_string_is_null || engine == MigrationEngine::Oracle
}

fn decimal_format(scale: u8) -> String {
    if scale == 0 {
        "FM99999999999999999999999999999999999990".to_string()
    } else {
        format!(
            "FM99999999999999999999999999999999999990.{}",
            "0".repeat(usize::from(scale))
        )
    }
}

fn sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_and_float_precision_are_explicit() {
        let policy = CanonicalizationPolicy::default();
        let decimal = CanonicalColumn::new("amount", CanonicalType::Decimal { scale: 2 });
        let float = CanonicalColumn::new("score", CanonicalType::Float { precision: 4 });

        assert!(
            canonical_cell_sql(MigrationEngine::Postgres, &decimal, &policy)
                .contains("DECIMAL(38,2)")
        );
        assert!(
            canonical_cell_sql(MigrationEngine::MySql, &float, &policy).contains("ROUND(score, 4)")
        );
    }

    #[test]
    fn oracle_empty_string_is_null_and_timestamp_is_padded() {
        let policy = CanonicalizationPolicy::default();
        let text = CanonicalColumn::new("name", CanonicalType::Text);
        let timestamp = CanonicalColumn::new(
            "updated_at",
            CanonicalType::Timestamp {
                fractional_digits: 3,
                mode: TimestampMode::Naive,
            },
        );

        assert!(canonical_cell_sql(MigrationEngine::Oracle, &text, &policy).contains("NULLIF"));
        assert!(
            canonical_cell_sql(MigrationEngine::Oracle, &timestamp, &policy)
                .contains("TIMESTAMP(3)")
        );
    }

    #[test]
    fn row_encoding_is_length_prefixed_to_avoid_delimiter_collisions() {
        let columns = vec![
            CanonicalColumn::new("id", CanonicalType::Integer),
            CanonicalColumn::new("payload", CanonicalType::Text),
        ];
        let sql = canonical_row_sql(
            MigrationEngine::Snowflake,
            &columns,
            &CanonicalizationPolicy::default(),
        );

        assert!(sql.contains("LENGTH"));
        assert!(sql.contains("'V'"));
        assert!(sql.contains("__IRODORI_NULL__"));
    }

    #[test]
    fn bytes_are_rendered_as_upper_hex() {
        let policy = CanonicalizationPolicy::default();
        let column = CanonicalColumn::new("raw_value", CanonicalType::Bytes);

        assert!(
            canonical_cell_sql(MigrationEngine::Postgres, &column, &policy)
                .contains("UPPER(ENCODE")
        );
        assert!(
            canonical_cell_sql(MigrationEngine::Snowflake, &column, &policy).contains("HEX_ENCODE")
        );
    }
}
