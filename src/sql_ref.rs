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
    engine.dialect().quote_identifier_if_needed(value.trim())
}

fn qualified_ref(engine: MigrationEngine, value: &str) -> String {
    value
        .split('.')
        .map(|part| identifier_ref(engine, part))
        .collect::<Vec<_>>()
        .join(".")
}
