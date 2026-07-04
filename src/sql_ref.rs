use std::error::Error;
use std::fmt;

use crate::dialect::SqlDialect;
use crate::plan::MigrationEngine;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TableRef {
    pub catalog: Option<String>,
    pub schema: Option<String>,
    pub name: String,
}

impl TableRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            catalog: None,
            schema: None,
            name: name.into(),
        }
    }

    pub fn qualified(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            catalog: None,
            schema: Some(schema.into()),
            name: name.into(),
        }
    }

    pub fn catalog_schema(
        catalog: impl Into<String>,
        schema: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            catalog: Some(catalog.into()),
            schema: Some(schema.into()),
            name: name.into(),
        }
    }

    pub fn parse_dotted(value: &str) -> Result<Self, TableRefParseError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(TableRefParseError::Empty);
        }
        let parts = trimmed.split('.').map(str::trim).collect::<Vec<_>>();
        if parts.iter().any(|part| part.is_empty()) {
            return Err(TableRefParseError::EmptyPart);
        }
        match parts.as_slice() {
            [name] => Ok(Self::new(*name)),
            [schema, name] => Ok(Self::qualified(*schema, *name)),
            [catalog, schema, name] => Ok(Self::catalog_schema(*catalog, *schema, *name)),
            _ => Err(TableRefParseError::TooManyParts),
        }
    }

    pub fn render(&self, dialect: &dyn SqlDialect) -> String {
        let mut parts = Vec::new();
        if let Some(catalog) = self.catalog.as_deref() {
            parts.push(catalog);
        }
        if let Some(schema) = self.schema.as_deref() {
            parts.push(schema);
        }
        parts.push(self.name.as_str());
        dialect.quote_qualified_identifier_if_needed(&parts)
    }

    pub fn render_for_engine(&self, engine: MigrationEngine) -> String {
        self.render(engine.dialect())
    }

    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty()
    }
}

impl fmt::Display for TableRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(catalog) = &self.catalog {
            write!(f, "{catalog}.")?;
        }
        if let Some(schema) = &self.schema {
            write!(f, "{schema}.")?;
        }
        write!(f, "{}", self.name)
    }
}

impl From<&str> for TableRef {
    fn from(value: &str) -> Self {
        Self::parse_dotted(value).unwrap_or_else(|_| Self::new(value.trim()))
    }
}

impl From<String> for TableRef {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableRefParseError {
    Empty,
    EmptyPart,
    TooManyParts,
}

impl fmt::Display for TableRefParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "table reference is empty"),
            Self::EmptyPart => write!(f, "table reference contains an empty part"),
            Self::TooManyParts => write!(f, "table reference has more than three parts"),
        }
    }
}

impl Error for TableRefParseError {}

pub(crate) fn table_ref(engine: MigrationEngine, name: &str) -> String {
    TableRef::from(name).render_for_engine(engine)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_ref_renders_dotted_parts_with_dialect_quoting() {
        let table = TableRef::parse_dotted("warehouse.order").expect("table");

        assert_eq!(
            table.render_for_engine(MigrationEngine::Postgres),
            "warehouse.\"order\""
        );
        assert_eq!(
            table.render_for_engine(MigrationEngine::MySql),
            "warehouse.`order`"
        );
    }

    #[test]
    fn table_ref_can_represent_literal_dot_in_name() {
        let table = TableRef::new("tenant.2026.orders");

        assert_eq!(
            table.render_for_engine(MigrationEngine::Postgres),
            "\"tenant.2026.orders\""
        );
    }

    #[test]
    fn dotted_parser_fails_closed_for_ambiguous_values() {
        assert_eq!(TableRef::parse_dotted(""), Err(TableRefParseError::Empty));
        assert_eq!(
            TableRef::parse_dotted("public..orders"),
            Err(TableRefParseError::EmptyPart)
        );
        assert_eq!(
            TableRef::parse_dotted("a.b.c.d"),
            Err(TableRefParseError::TooManyParts)
        );
    }
}
