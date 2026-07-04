use std::io;

use serde::{Deserialize, Serialize};

use super::{dialect_uses_backslash_escapes, sql_string_literal, OwnedCell};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferredColumn {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InferredType {
    Null,
    Bool,
    Integer,
    Float,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportColumn {
    pub source_name: String,
    pub target_name: String,
    pub inferred_type: InferredType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportPreview {
    pub columns: Vec<ImportColumn>,
    pub rows: Vec<Vec<OwnedCell>>,
    pub total_rows_seen: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMapping {
    pub source_name: String,
    pub target_name: Option<String>,
}

impl ImportPreview {
    pub fn mapped_columns(&self, mapping: &[ColumnMapping]) -> Vec<String> {
        self.columns
            .iter()
            .filter_map(|column| {
                if let Some(item) = mapping
                    .iter()
                    .find(|item| item.source_name == column.source_name)
                {
                    item.target_name.clone()
                } else {
                    Some(column.target_name.clone())
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPreviewOptions {
    pub max_rows: usize,
}

impl Default for ImportPreviewOptions {
    fn default() -> Self {
        Self { max_rows: 100 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelimitedImportOptions {
    pub delimiter: u8,
    pub quote: u8,
    pub has_header: bool,
    pub null_values: Vec<String>,
    pub preview: ImportPreviewOptions,
}

impl DelimitedImportOptions {
    pub fn csv() -> Self {
        Self {
            delimiter: b',',
            quote: b'"',
            has_header: true,
            null_values: vec![String::new()],
            preview: ImportPreviewOptions::default(),
        }
    }

    pub fn tsv() -> Self {
        Self {
            delimiter: b'\t',
            quote: b'"',
            has_header: true,
            null_values: vec![String::new()],
            preview: ImportPreviewOptions::default(),
        }
    }

    pub fn with_header(mut self, has_header: bool) -> Self {
        self.has_header = has_header;
        self
    }

    pub fn with_max_preview_rows(mut self, max_rows: usize) -> Self {
        self.preview.max_rows = max_rows;
        self
    }
}

pub fn preview_json(input: &str, options: ImportPreviewOptions) -> io::Result<ImportPreview> {
    let value = serde_json::from_str::<serde_json::Value>(input)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let rows = match value {
        serde_json::Value::Array(rows) => rows,
        serde_json::Value::Object(_) => vec![value],
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "JSON import expects an object or array of objects",
            ));
        }
    };
    preview_json_values(rows.into_iter().map(Ok), options.max_rows)
}

pub fn preview_ndjson(input: &str, options: ImportPreviewOptions) -> io::Result<ImportPreview> {
    preview_json_values(
        input.lines().enumerate().filter_map(|(line_index, line)| {
            if line.trim().is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str::<serde_json::Value>(line).map_err(|error| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("invalid NDJSON at line {}: {error}", line_index + 1),
                        )
                    }),
                )
            }
        }),
        options.max_rows,
    )
}

pub fn preview_delimited<R: io::Read>(
    reader: R,
    options: DelimitedImportOptions,
) -> io::Result<ImportPreview> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(options.delimiter)
        .quote(options.quote)
        .has_headers(options.has_header)
        .from_reader(reader);

    let headers = if options.has_header {
        rdr.headers()?
            .iter()
            .map(|s| strip_utf8_bom(s).to_string())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut record = csv::StringRecord::new();
    let mut rows = Vec::new();
    let mut width = headers.len();
    let mut inferred_types = vec![InferredType::Null; width];
    let mut total_rows_seen = 0;
    while rdr.read_record(&mut record)? {
        if width == 0 || record.len() > width {
            width = record.len();
            inferred_types.resize(width, InferredType::Null);
        }
        total_rows_seen += 1;
        for (index, inferred_type) in inferred_types.iter_mut().enumerate().take(width) {
            let cell = infer_delimited_cell(
                record.get(index).map(strip_utf8_bom).unwrap_or_default(),
                &options.null_values,
            )?;
            *inferred_type = merge_types(*inferred_type, cell_type(&cell));
        }
        if rows.len() < options.preview.max_rows {
            rows.push(
                (0..width)
                    .map(|index| {
                        infer_delimited_cell(
                            record.get(index).map(strip_utf8_bom).unwrap_or_default(),
                            &options.null_values,
                        )
                    })
                    .collect::<io::Result<Vec<_>>>()?,
            );
        }
    }

    let mut headers = if options.has_header {
        headers
    } else {
        (0..width)
            .map(|index| format!("column_{}", index + 1))
            .collect()
    };
    if headers.len() < width {
        headers.extend((headers.len()..width).map(|index| format!("column_{}", index + 1)));
    }
    for row in &mut rows {
        row.resize(width, OwnedCell::Null);
    }
    Ok(build_preview_with_types(
        headers,
        rows,
        inferred_types,
        total_rows_seen,
        total_rows_seen > options.preview.max_rows,
    ))
}

pub fn infer_csv_schema<R: io::Read>(
    reader: R,
    delimiter: u8,
    has_header: bool,
) -> io::Result<Vec<InferredColumn>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(has_header)
        .from_reader(reader);

    let headers = if has_header {
        rdr.headers()?
            .iter()
            .map(|s| strip_utf8_bom(s).to_string())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut record = csv::StringRecord::new();
    let mut sampled_rows = Vec::new();
    let mut num_cols = headers.len();

    while rdr.read_record(&mut record)? {
        if num_cols == 0 {
            num_cols = record.len();
        }
        sampled_rows.push(record.clone());
        if sampled_rows.len() >= 100 {
            break;
        }
    }

    let headers = if has_header {
        headers
    } else {
        (0..num_cols).map(|i| format!("col_{}", i + 1)).collect()
    };

    let mut inferred = Vec::new();
    for col_idx in 0..num_cols {
        let mut inferred_type = InferredType::Null;

        for row in &sampled_rows {
            if let Some(val) = row.get(col_idx) {
                let cell = infer_delimited_cell(strip_utf8_bom(val), &[String::new()])?;
                inferred_type = merge_types(inferred_type, cell_type(&cell));
            }
        }

        let dtype = if inferred_type == InferredType::Null {
            "text"
        } else if inferred_type == InferredType::Bool {
            "boolean"
        } else if inferred_type == InferredType::Integer {
            "integer"
        } else if inferred_type == InferredType::Float {
            "double"
        } else {
            "text"
        };

        inferred.push(InferredColumn {
            name: headers
                .get(col_idx)
                .cloned()
                .unwrap_or_else(|| format!("col_{}", col_idx + 1)),
            data_type: dtype.to_string(),
        });
    }

    Ok(inferred)
}

pub fn generate_inserts_from_csv<R: io::Read, W: io::Write>(
    reader: R,
    delimiter: u8,
    has_header: bool,
    table_name: &str,
    mut sql_writer: W,
    dialect: &dyn crate::dialect::SqlDialect,
) -> io::Result<usize> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(has_header)
        .from_reader(reader);

    let headers = if has_header {
        rdr.headers()?
            .iter()
            .map(|s| strip_utf8_bom(s).to_string())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut record = csv::StringRecord::new();
    let mut num_cols = headers.len();
    let mut count = 0;

    let quoted_table = dialect.quote_identifier(table_name);
    let backslash_escapes = dialect_uses_backslash_escapes(dialect);

    while rdr.read_record(&mut record)? {
        if num_cols == 0 {
            num_cols = record.len();
        } else if record.len() != num_cols {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "CSV row has {} fields but expected {num_cols}",
                    record.len()
                ),
            ));
        }
        let col_names = if has_header {
            headers.clone()
        } else {
            (0..num_cols).map(|i| format!("col_{}", i + 1)).collect()
        };

        let quoted_cols: Vec<String> = col_names
            .iter()
            .map(|c| dialect.quote_identifier(c))
            .collect();
        let cols_str = quoted_cols.join(", ");

        let mut vals = Vec::new();
        for val in record.iter() {
            vals.push(delimited_value_to_sql(val, backslash_escapes)?);
        }
        let vals_str = vals.join(", ");

        writeln!(
            sql_writer,
            "INSERT INTO {} ({}) VALUES ({});",
            quoted_table, cols_str, vals_str
        )?;
        count += 1;
    }

    Ok(count)
}

fn delimited_value_to_sql(value: &str, backslash_escapes: bool) -> io::Result<String> {
    let value = strip_utf8_bom(value);
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        Ok("NULL".to_string())
    } else if trimmed.eq_ignore_ascii_case("true") {
        Ok("TRUE".to_string())
    } else if trimmed.eq_ignore_ascii_case("false") {
        Ok("FALSE".to_string())
    } else if is_lossy_integer_text(trimmed) {
        Ok(sql_string_literal(trimmed, backslash_escapes))
    } else if is_integer_like(trimmed) {
        trimmed
            .parse::<i64>()
            .map(|_| trimmed.to_string())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "integer is out of range"))
    } else if let Ok(parsed) = trimmed.parse::<f64>() {
        if parsed.is_finite() {
            Ok(trimmed.to_string())
        } else {
            Err(non_finite_import_error())
        }
    } else {
        Ok(sql_string_literal(trimmed, backslash_escapes))
    }
}

fn preview_json_values(
    values: impl Iterator<Item = io::Result<serde_json::Value>>,
    max_rows: usize,
) -> io::Result<ImportPreview> {
    let mut headers = Vec::<String>::new();
    let mut rows = Vec::<Vec<(String, OwnedCell)>>::new();
    let mut inferred_types = Vec::<InferredType>::new();
    let mut total_rows_seen = 0;

    for value in values {
        let value = value?;
        let serde_json::Value::Object(map) = value else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "JSON import expects only objects as rows",
            ));
        };
        total_rows_seen += 1;
        let keep_row = total_rows_seen <= max_rows;
        let mut row = Vec::new();
        for (key, value) in map {
            let index = match headers.iter().position(|existing| existing == &key) {
                Some(index) => index,
                None => {
                    headers.push(key.clone());
                    inferred_types.push(InferredType::Null);
                    headers.len() - 1
                }
            };
            let cell = owned_cell_from_json(value)?;
            inferred_types[index] = merge_types(inferred_types[index], cell_type(&cell));
            if keep_row {
                row.push((key, cell));
            }
        }
        if total_rows_seen <= max_rows {
            rows.push(row);
        }
    }

    let preview_rows = rows
        .into_iter()
        .map(|row| {
            headers
                .iter()
                .map(|header| {
                    row.iter()
                        .find(|(key, _)| key == header)
                        .map(|(_, value)| value.clone())
                        .unwrap_or(OwnedCell::Null)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    Ok(build_preview_with_types(
        headers,
        preview_rows,
        inferred_types,
        total_rows_seen,
        total_rows_seen > max_rows,
    ))
}

fn build_preview_with_types(
    headers: Vec<String>,
    rows: Vec<Vec<OwnedCell>>,
    inferred_types: Vec<InferredType>,
    total_rows_seen: usize,
    truncated: bool,
) -> ImportPreview {
    let columns = headers
        .into_iter()
        .enumerate()
        .map(|(index, source_name)| {
            let inferred_type = inferred_types
                .get(index)
                .copied()
                .unwrap_or_else(|| infer_column_type(rows.iter().filter_map(|row| row.get(index))));
            ImportColumn {
                target_name: sanitize_column_name(&source_name, index),
                source_name,
                inferred_type,
            }
        })
        .collect();
    ImportPreview {
        columns,
        rows,
        total_rows_seen,
        truncated,
    }
}

fn owned_cell_from_json(value: serde_json::Value) -> io::Result<OwnedCell> {
    Ok(match value {
        serde_json::Value::Null => OwnedCell::Null,
        serde_json::Value::Bool(value) => OwnedCell::Bool(value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                OwnedCell::Integer(value)
            } else if value.as_u64().is_some() {
                OwnedCell::Text(value.to_string())
            } else if let Some(value) = value.as_f64() {
                if value.is_finite() {
                    OwnedCell::Float(value)
                } else {
                    return Err(non_finite_import_error());
                }
            } else {
                OwnedCell::Text(value.to_string())
            }
        }
        serde_json::Value::String(value) => OwnedCell::Text(value),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            OwnedCell::Text(value.to_string())
        }
    })
}

fn infer_delimited_cell(value: &str, null_values: &[String]) -> io::Result<OwnedCell> {
    let value = strip_utf8_bom(value);
    if null_values.iter().any(|null_value| null_value == value) {
        return Ok(OwnedCell::Null);
    }
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        Ok(OwnedCell::Bool(true))
    } else if trimmed.eq_ignore_ascii_case("false") {
        Ok(OwnedCell::Bool(false))
    } else if is_lossy_integer_text(trimmed) {
        Ok(OwnedCell::Text(value.to_string()))
    } else if is_integer_like(trimmed) {
        trimmed
            .parse::<i64>()
            .map(OwnedCell::Integer)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "integer is out of range"))
    } else if let Ok(parsed) = trimmed.parse::<f64>() {
        if parsed.is_finite() {
            Ok(OwnedCell::Float(parsed))
        } else {
            Err(non_finite_import_error())
        }
    } else {
        Ok(OwnedCell::Text(value.to_string()))
    }
}

fn infer_column_type<'a>(cells: impl Iterator<Item = &'a OwnedCell>) -> InferredType {
    cells.fold(InferredType::Null, |current, cell| {
        merge_types(current, cell_type(cell))
    })
}

fn cell_type(cell: &OwnedCell) -> InferredType {
    match cell {
        OwnedCell::Null => InferredType::Null,
        OwnedCell::Bool(_) => InferredType::Bool,
        OwnedCell::Integer(_) => InferredType::Integer,
        OwnedCell::Float(_) => InferredType::Float,
        OwnedCell::Text(_) => InferredType::Text,
    }
}

fn merge_types(left: InferredType, right: InferredType) -> InferredType {
    match (left, right) {
        (InferredType::Null, other) | (other, InferredType::Null) => other,
        (InferredType::Integer, InferredType::Float)
        | (InferredType::Float, InferredType::Integer) => InferredType::Float,
        (same_left, same_right) if same_left == same_right => same_left,
        _ => InferredType::Text,
    }
}

fn strip_utf8_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

fn is_lossy_integer_text(value: &str) -> bool {
    is_integer_like(value) && (has_integer_leading_zero(value) || value.parse::<i64>().is_err())
}

fn is_integer_like(value: &str) -> bool {
    let digits = value
        .strip_prefix(['+', '-'])
        .filter(|digits| !digits.is_empty())
        .unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn has_integer_leading_zero(value: &str) -> bool {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    digits.len() > 1 && digits.starts_with('0')
}

fn non_finite_import_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "non-finite floating-point values cannot be imported",
    )
}

fn sanitize_column_name(value: &str, index: usize) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        format!("column_{}", index + 1)
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("column_{out}")
    } else {
        out
    }
}
