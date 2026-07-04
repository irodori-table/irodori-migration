//! ADV-001 — schema comparison and migration preview.
//!
//! A pure, source-agnostic schema model plus a structural diff and a migration
//! **preview** generator. [`diff_schemas`] turns two [`Schema`] snapshots into a
//! readable [`SchemaDiff`] (tables/columns/indexes added, dropped, or altered),
//! and [`SchemaDiff::to_migration`] renders dialect-quoted DDL for it.
//!
//! Safe-apply contract: this module only *generates* SQL — it never executes it.
//! Every statement is tagged [`MigrationStatement::destructive`] (a `DROP` that can
//! lose data), so a caller can preview the change set, require explicit
//! confirmation for destructive steps, and run the script inside a transaction on
//! dialects that support transactional DDL. Reviewing the preview before applying
//! is the intended workflow.

use crate::dialect::SqlDialect;

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
}

impl Column {
    pub fn new(name: impl Into<String>, data_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data_type: data_type.into(),
            nullable: true,
            default: None,
        }
    }

    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyConstraint {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckConstraint {
    pub name: String,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueConstraint {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableConstraint {
    ForeignKey(ForeignKeyConstraint),
    Check(CheckConstraint),
    Unique(UniqueConstraint),
}

impl TableConstraint {
    pub fn name(&self) -> &str {
        match self {
            TableConstraint::ForeignKey(constraint) => &constraint.name,
            TableConstraint::Check(constraint) => &constraint.name,
            TableConstraint::Unique(constraint) => &constraint.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameHint {
    Table {
        from: String,
        to: String,
    },
    Column {
        table: String,
        from: String,
        to: String,
    },
    Index {
        table: String,
        from: String,
        to: String,
    },
    Constraint {
        table: String,
        from: String,
        to: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub primary_key: Vec<String>,
    pub indexes: Vec<Index>,
    pub constraints: Vec<TableConstraint>,
}

impl Table {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn with_columns(mut self, columns: Vec<Column>) -> Self {
        self.columns = columns;
        self
    }

    pub fn with_primary_key(mut self, primary_key: Vec<String>) -> Self {
        self.primary_key = primary_key;
        self
    }

    pub fn with_indexes(mut self, indexes: Vec<Index>) -> Self {
        self.indexes = indexes;
        self
    }

    pub fn with_constraints(mut self, constraints: Vec<TableConstraint>) -> Self {
        self.constraints = constraints;
        self
    }

    fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    fn index(&self, name: &str) -> Option<&Index> {
        self.indexes.iter().find(|i| i.name == name)
    }

    fn constraint(&self, name: &str) -> Option<&TableConstraint> {
        self.constraints
            .iter()
            .find(|constraint| constraint.name() == name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    pub tables: Vec<Table>,
    pub rename_hints: Vec<RenameHint>,
}

impl Schema {
    pub fn new(tables: Vec<Table>) -> Self {
        Self {
            tables,
            rename_hints: Vec::new(),
        }
    }

    pub fn with_rename_hints(mut self, rename_hints: Vec<RenameHint>) -> Self {
        self.rename_hints = rename_hints;
        self
    }

    fn table(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.name == name)
    }

    fn table_rename_from(&self, to: &str) -> Option<&str> {
        self.rename_hints.iter().find_map(|hint| match hint {
            RenameHint::Table { from, to: target } if target == to => Some(from.as_str()),
            _ => None,
        })
    }

    fn table_rename_to(&self, from: &str) -> Option<&str> {
        self.rename_hints.iter().find_map(|hint| match hint {
            RenameHint::Table { from: source, to } if source == from => Some(to.as_str()),
            _ => None,
        })
    }
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnChange {
    Type {
        from: String,
        to: String,
    },
    Nullability {
        nullable: bool,
    },
    Default {
        from: Option<String>,
        to: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlteredColumn {
    /// The column as it should become (used by single-statement `MODIFY` dialects).
    pub column: Column,
    pub changes: Vec<ColumnChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryKeyChange {
    pub from: Vec<String>,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlteredTable {
    pub name: String,
    pub renamed_from: Option<String>,
    pub renamed_columns: Vec<Rename>,
    pub added_columns: Vec<Column>,
    pub dropped_columns: Vec<String>,
    pub altered_columns: Vec<AlteredColumn>,
    pub primary_key_change: Option<PrimaryKeyChange>,
    pub renamed_indexes: Vec<Rename>,
    pub added_indexes: Vec<Index>,
    pub dropped_indexes: Vec<String>,
    pub renamed_constraints: Vec<Rename>,
    pub added_constraints: Vec<TableConstraint>,
    pub dropped_constraints: Vec<TableConstraint>,
}

impl AlteredTable {
    fn is_empty(&self) -> bool {
        self.renamed_from.is_none()
            && self.renamed_columns.is_empty()
            && self.added_columns.is_empty()
            && self.dropped_columns.is_empty()
            && self.altered_columns.is_empty()
            && self.primary_key_change.is_none()
            && self.renamed_indexes.is_empty()
            && self.added_indexes.is_empty()
            && self.dropped_indexes.is_empty()
            && self.renamed_constraints.is_empty()
            && self.added_constraints.is_empty()
            && self.dropped_constraints.is_empty()
    }

    fn has_destructive_changes(&self) -> bool {
        !self.dropped_columns.is_empty()
            || !self.dropped_indexes.is_empty()
            || self
                .altered_columns
                .iter()
                .any(|column| column.changes.iter().any(ColumnChange::is_destructive))
            || self
                .primary_key_change
                .as_ref()
                .is_some_and(|change| !change.from.is_empty() && change.from != change.to)
            || !self.dropped_constraints.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaDiff {
    pub added_tables: Vec<Table>,
    pub dropped_tables: Vec<String>,
    pub altered_tables: Vec<AlteredTable>,
}

impl SchemaDiff {
    pub fn is_empty(&self) -> bool {
        self.added_tables.is_empty()
            && self.dropped_tables.is_empty()
            && self.altered_tables.is_empty()
    }

    /// Whether the diff contains a destructive or safety-lowering change.
    pub fn has_destructive_changes(&self) -> bool {
        !self.dropped_tables.is_empty()
            || self
                .altered_tables
                .iter()
                .any(AlteredTable::has_destructive_changes)
    }

    /// A short, human-readable change set for the preview header.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "no schema changes".to_string();
        }
        let mut parts = Vec::new();
        if !self.added_tables.is_empty() {
            parts.push(format!("+{} table(s)", self.added_tables.len()));
        }
        if !self.dropped_tables.is_empty() {
            parts.push(format!("-{} table(s)", self.dropped_tables.len()));
        }
        for table in &self.altered_tables {
            let mut bits = Vec::new();
            if !table.added_columns.is_empty() {
                bits.push(format!("+{}col", table.added_columns.len()));
            }
            if !table.dropped_columns.is_empty() {
                bits.push(format!("-{}col", table.dropped_columns.len()));
            }
            if !table.altered_columns.is_empty() {
                bits.push(format!("~{}col", table.altered_columns.len()));
            }
            if !table.added_indexes.is_empty() {
                bits.push(format!("+{}idx", table.added_indexes.len()));
            }
            if !table.dropped_indexes.is_empty() {
                bits.push(format!("-{}idx", table.dropped_indexes.len()));
            }
            if table.primary_key_change.is_some() {
                bits.push("~pk".to_string());
            }
            if !table.added_constraints.is_empty() {
                bits.push(format!("+{}constraint", table.added_constraints.len()));
            }
            if !table.dropped_constraints.is_empty() {
                bits.push(format!("-{}constraint", table.dropped_constraints.len()));
            }
            parts.push(format!("{} ({})", table.name, bits.join(", ")));
        }
        parts.join("; ")
    }
}

/// Diff `old` into `new`: what would have to change to turn `old` into `new`.
pub fn diff_schemas(old: &Schema, new: &Schema) -> SchemaDiff {
    let mut diff = SchemaDiff::default();

    for new_table in &new.tables {
        if old.table(&new_table.name).is_none()
            && new
                .table_rename_from(&new_table.name)
                .and_then(|old_name| old.table(old_name))
                .is_none()
        {
            diff.added_tables.push(new_table.clone());
        }
    }
    for old_table in &old.tables {
        if new.table(&old_table.name).is_none()
            && new
                .table_rename_to(&old_table.name)
                .and_then(|new_name| new.table(new_name))
                .is_none()
        {
            diff.dropped_tables.push(old_table.name.clone());
        }
    }
    for new_table in &new.tables {
        let old_table = old.table(&new_table.name).or_else(|| {
            new.table_rename_from(&new_table.name)
                .and_then(|old_name| old.table(old_name))
        });
        if let Some(old_table) = old_table {
            let mut altered = diff_table(old_table, new_table, &new.rename_hints);
            if old_table.name != new_table.name {
                altered.renamed_from = Some(old_table.name.clone());
            }
            if !altered.is_empty() {
                diff.altered_tables.push(altered);
            }
        }
    }
    diff
}

fn diff_table(old: &Table, new: &Table, rename_hints: &[RenameHint]) -> AlteredTable {
    let mut altered = AlteredTable {
        name: new.name.clone(),
        renamed_from: None,
        renamed_columns: Vec::new(),
        added_columns: Vec::new(),
        dropped_columns: Vec::new(),
        altered_columns: Vec::new(),
        primary_key_change: None,
        renamed_indexes: Vec::new(),
        added_indexes: Vec::new(),
        dropped_indexes: Vec::new(),
        renamed_constraints: Vec::new(),
        added_constraints: Vec::new(),
        dropped_constraints: Vec::new(),
    };

    for new_column in &new.columns {
        match old.column(&new_column.name) {
            None => altered.added_columns.push(new_column.clone()),
            Some(old_column) => {
                let changes = diff_column(old_column, new_column);
                if !changes.is_empty() {
                    altered.altered_columns.push(AlteredColumn {
                        column: new_column.clone(),
                        changes,
                    });
                }
            }
        }
    }
    for new_column in &new.columns {
        if old.column(&new_column.name).is_some() {
            continue;
        }
        if let Some(old_name) =
            rename_from(rename_hints, RenameKind::Column, old, new, &new_column.name)
        {
            if let Some(old_column) = old.column(old_name) {
                altered
                    .added_columns
                    .retain(|column| column.name != new_column.name);
                altered.renamed_columns.push(Rename {
                    from: old_name.to_string(),
                    to: new_column.name.clone(),
                });
                let changes = diff_column(old_column, new_column);
                if !changes.is_empty() {
                    altered.altered_columns.push(AlteredColumn {
                        column: new_column.clone(),
                        changes,
                    });
                }
            }
        }
    }
    for old_column in &old.columns {
        if new.column(&old_column.name).is_none()
            && rename_to(rename_hints, RenameKind::Column, old, new, &old_column.name).is_none()
        {
            altered.dropped_columns.push(old_column.name.clone());
        }
    }
    if old.primary_key != new.primary_key {
        altered.primary_key_change = Some(PrimaryKeyChange {
            from: old.primary_key.clone(),
            to: new.primary_key.clone(),
        });
    }
    for new_index in &new.indexes {
        match old.index(&new_index.name) {
            // Unchanged: nothing to do.
            Some(existing) if existing == new_index => {}
            // Changed definition: drop and recreate.
            Some(_) => {
                altered.dropped_indexes.push(new_index.name.clone());
                altered.added_indexes.push(new_index.clone());
            }
            // Brand new index.
            None => altered.added_indexes.push(new_index.clone()),
        }
    }
    for new_index in &new.indexes {
        if old.index(&new_index.name).is_some() {
            continue;
        }
        if let Some(old_name) =
            rename_from(rename_hints, RenameKind::Index, old, new, &new_index.name)
        {
            if let Some(old_index) = old.index(old_name) {
                altered
                    .added_indexes
                    .retain(|index| index.name != new_index.name);
                altered.renamed_indexes.push(Rename {
                    from: old_name.to_string(),
                    to: new_index.name.clone(),
                });
                if old_index.columns != new_index.columns || old_index.unique != new_index.unique {
                    altered.dropped_indexes.push(new_index.name.clone());
                    altered.added_indexes.push(new_index.clone());
                }
            }
        }
    }
    for old_index in &old.indexes {
        if new.index(&old_index.name).is_none()
            && rename_to(rename_hints, RenameKind::Index, old, new, &old_index.name).is_none()
        {
            altered.dropped_indexes.push(old_index.name.clone());
        }
    }
    for new_constraint in &new.constraints {
        match old.constraint(new_constraint.name()) {
            Some(existing) if existing == new_constraint => {}
            Some(existing) => {
                altered.dropped_constraints.push(existing.clone());
                altered.added_constraints.push(new_constraint.clone());
            }
            None => altered.added_constraints.push(new_constraint.clone()),
        }
    }
    for new_constraint in &new.constraints {
        if old.constraint(new_constraint.name()).is_some() {
            continue;
        }
        if let Some(old_name) = rename_from(
            rename_hints,
            RenameKind::Constraint,
            old,
            new,
            new_constraint.name(),
        ) {
            if let Some(old_constraint) = old.constraint(old_name) {
                altered
                    .added_constraints
                    .retain(|constraint| constraint.name() != new_constraint.name());
                altered.renamed_constraints.push(Rename {
                    from: old_name.to_string(),
                    to: new_constraint.name().to_string(),
                });
                if !same_constraint_body(old_constraint, new_constraint) {
                    altered.dropped_constraints.push(new_constraint.clone());
                    altered.added_constraints.push(new_constraint.clone());
                }
            }
        }
    }
    for old_constraint in &old.constraints {
        if new.constraint(old_constraint.name()).is_none()
            && rename_to(
                rename_hints,
                RenameKind::Constraint,
                old,
                new,
                old_constraint.name(),
            )
            .is_none()
        {
            altered.dropped_constraints.push(old_constraint.clone());
        }
    }
    altered
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenameKind {
    Column,
    Index,
    Constraint,
}

fn rename_from<'a>(
    hints: &'a [RenameHint],
    kind: RenameKind,
    old: &Table,
    new: &Table,
    to: &str,
) -> Option<&'a str> {
    hints.iter().find_map(|hint| match (kind, hint) {
        (
            RenameKind::Column,
            RenameHint::Column {
                table,
                from,
                to: target,
            },
        )
        | (
            RenameKind::Index,
            RenameHint::Index {
                table,
                from,
                to: target,
            },
        )
        | (
            RenameKind::Constraint,
            RenameHint::Constraint {
                table,
                from,
                to: target,
            },
        ) if target == to && (table == &old.name || table == &new.name) => Some(from.as_str()),
        _ => None,
    })
}

fn rename_to<'a>(
    hints: &'a [RenameHint],
    kind: RenameKind,
    old: &Table,
    new: &Table,
    from: &str,
) -> Option<&'a str> {
    hints.iter().find_map(|hint| match (kind, hint) {
        (
            RenameKind::Column,
            RenameHint::Column {
                table,
                from: source,
                to,
            },
        )
        | (
            RenameKind::Index,
            RenameHint::Index {
                table,
                from: source,
                to,
            },
        )
        | (
            RenameKind::Constraint,
            RenameHint::Constraint {
                table,
                from: source,
                to,
            },
        ) if source == from && (table == &old.name || table == &new.name) => Some(to.as_str()),
        _ => None,
    })
}

fn same_constraint_body(old: &TableConstraint, new: &TableConstraint) -> bool {
    match (old, new) {
        (TableConstraint::ForeignKey(old), TableConstraint::ForeignKey(new)) => {
            old.columns == new.columns
                && old.referenced_table == new.referenced_table
                && old.referenced_columns == new.referenced_columns
                && old.on_delete == new.on_delete
                && old.on_update == new.on_update
        }
        (TableConstraint::Check(old), TableConstraint::Check(new)) => {
            old.expression == new.expression
        }
        (TableConstraint::Unique(old), TableConstraint::Unique(new)) => old.columns == new.columns,
        _ => false,
    }
}

fn diff_column(old: &Column, new: &Column) -> Vec<ColumnChange> {
    let mut changes = Vec::new();
    if old.data_type != new.data_type {
        changes.push(ColumnChange::Type {
            from: old.data_type.clone(),
            to: new.data_type.clone(),
        });
    }
    if old.nullable != new.nullable {
        changes.push(ColumnChange::Nullability {
            nullable: new.nullable,
        });
    }
    if old.default != new.default {
        changes.push(ColumnChange::Default {
            from: old.default.clone(),
            to: new.default.clone(),
        });
    }
    changes
}

impl ColumnChange {
    fn is_destructive(&self) -> bool {
        match self {
            ColumnChange::Type { from, to } => is_type_narrowing(from, to),
            ColumnChange::Nullability { nullable } => !nullable,
            ColumnChange::Default { .. } => false,
        }
    }
}

fn is_type_narrowing(from: &str, to: &str) -> bool {
    let from = TypeShape::parse(from);
    let to = TypeShape::parse(to);
    match (&from.kind, &to.kind) {
        (TypeKind::String, TypeKind::String) => match (from.width, to.width) {
            (_, None) => false,
            (Some(from_width), Some(to_width)) => to_width < from_width,
            (None, Some(_)) => true,
        },
        (TypeKind::Integer, TypeKind::Integer)
        | (TypeKind::Decimal, TypeKind::Decimal)
        | (TypeKind::Float, TypeKind::Float) => {
            let rank_narrowed = to.rank < from.rank;
            let width_narrowed = match (from.width, to.width) {
                (Some(from_width), Some(to_width)) => to_width < from_width,
                (None, Some(_)) => true,
                _ => false,
            };
            let scale_narrowed = match (from.scale, to.scale) {
                (Some(from_scale), Some(to_scale)) => to_scale < from_scale,
                (Some(_), None) => true,
                _ => false,
            };
            rank_narrowed || width_narrowed || scale_narrowed
        }
        (TypeKind::Unknown, _) | (_, TypeKind::Unknown) => true,
        (from_kind, to_kind) => from_kind != to_kind,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    kind: TypeKind,
    rank: u8,
    width: Option<u32>,
    scale: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeKind {
    String,
    Integer,
    Decimal,
    Float,
    Boolean,
    Temporal,
    Binary,
    Unknown,
}

impl TypeShape {
    fn parse(raw: &str) -> Self {
        let normalized = raw.trim().to_ascii_lowercase();
        let base = normalized
            .split_once('(')
            .map(|(base, _)| base.trim())
            .unwrap_or(normalized.trim())
            .split_whitespace()
            .next()
            .unwrap_or("");
        let args = type_args(&normalized);
        match base {
            "char" | "character" | "nchar" => Self::string(args.first().copied()),
            "varchar" | "character varying" | "nvarchar" => Self::string(args.first().copied()),
            "text" | "tinytext" | "mediumtext" | "longtext" | "clob" => Self::string(None),
            "tinyint" => Self::integer(1),
            "smallint" | "int2" => Self::integer(2),
            "mediumint" => Self::integer(3),
            "integer" | "int" | "int4" | "serial" => Self::integer(4),
            "bigint" | "int8" | "bigserial" => Self::integer(8),
            "numeric" | "decimal" | "number" => Self {
                kind: TypeKind::Decimal,
                rank: 0,
                width: args.first().copied(),
                scale: args.get(1).copied(),
            },
            "real" | "float4" => Self::float(4),
            "double" | "float8" => Self::float(8),
            "float" => Self::float(args.first().copied().unwrap_or(8)),
            "bool" | "boolean" => Self::simple(TypeKind::Boolean),
            "date" | "time" | "timestamp" | "timestamptz" | "datetime" => {
                Self::simple(TypeKind::Temporal)
            }
            "bytea" | "blob" | "binary" | "varbinary" => Self::simple(TypeKind::Binary),
            _ => Self::simple(TypeKind::Unknown),
        }
    }

    fn simple(kind: TypeKind) -> Self {
        Self {
            kind,
            rank: 0,
            width: None,
            scale: None,
        }
    }

    fn string(width: Option<u32>) -> Self {
        Self {
            kind: TypeKind::String,
            rank: 0,
            width,
            scale: None,
        }
    }

    fn integer(rank: u8) -> Self {
        Self {
            kind: TypeKind::Integer,
            rank,
            width: None,
            scale: None,
        }
    }

    fn float(rank: u32) -> Self {
        Self {
            kind: TypeKind::Float,
            rank: rank.min(u8::MAX as u32) as u8,
            width: None,
            scale: None,
        }
    }
}

fn type_args(normalized: &str) -> Vec<u32> {
    normalized
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')').map(|(args, _)| args))
        .map(|args| {
            args.split(',')
                .filter_map(|arg| arg.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Migration preview
// ---------------------------------------------------------------------------

/// How a dialect alters an existing column. `Standard` is Postgres/ANSI-style
/// granular `ALTER COLUMN`; `MySql` rewrites the whole column with `MODIFY COLUMN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlterColumnStyle {
    Standard,
    MySql,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatement {
    pub sql: String,
    /// True for destructive or safety-lowering statements.
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedMigration {
    pub operation: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationScript {
    pub statements: Vec<MigrationStatement>,
    pub unsupported: Vec<UnsupportedMigration>,
}

impl MigrationScript {
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty() && self.unsupported.is_empty()
    }

    pub fn destructive_count(&self) -> usize {
        self.statements.iter().filter(|s| s.destructive).count()
    }

    pub fn has_unsupported(&self) -> bool {
        !self.unsupported.is_empty()
    }

    /// The full script as one newline-joined SQL string.
    pub fn to_sql(&self) -> String {
        self.statements
            .iter()
            .map(|statement| statement.sql.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl SchemaDiff {
    /// Render this diff as a dialect-quoted migration preview. Statements are
    /// ordered create-tables → drop-tables → per-altered-table changes; each is
    /// flagged destructive when it drops something.
    pub fn to_migration(
        &self,
        dialect: &dyn SqlDialect,
        style: AlterColumnStyle,
    ) -> MigrationScript {
        let mut statements = Vec::new();
        let mut unsupported = Vec::new();

        for table in &self.added_tables {
            statements.push(MigrationStatement {
                sql: create_table_sql(dialect, table),
                destructive: false,
            });
            for index in &table.indexes {
                statements.push(MigrationStatement {
                    sql: create_index_sql(dialect, &table.name, index),
                    destructive: false,
                });
            }
        }
        for name in &self.dropped_tables {
            statements.push(MigrationStatement {
                sql: format!("DROP TABLE {};", dialect.quote_identifier(name)),
                destructive: true,
            });
        }
        for table in &self.altered_tables {
            emit_altered_table(dialect, style, table, &mut statements, &mut unsupported);
        }

        MigrationScript {
            statements,
            unsupported,
        }
    }
}

fn create_table_sql(dialect: &dyn SqlDialect, table: &Table) -> String {
    let mut lines: Vec<String> = table
        .columns
        .iter()
        .map(|column| format!("  {}", column_definition(dialect, column)))
        .collect();
    if !table.primary_key.is_empty() {
        let keys = table
            .primary_key
            .iter()
            .map(|key| dialect.quote_identifier(key))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("  PRIMARY KEY ({keys})"));
    }
    for constraint in &table.constraints {
        lines.push(format!("  {}", constraint_definition(dialect, constraint)));
    }
    format!(
        "CREATE TABLE {} (\n{}\n);",
        dialect.quote_identifier(&table.name),
        lines.join(",\n")
    )
}

fn column_definition(dialect: &dyn SqlDialect, column: &Column) -> String {
    let mut def = format!(
        "{} {}",
        dialect.quote_identifier(&column.name),
        column.data_type
    );
    if !column.nullable {
        def.push_str(" NOT NULL");
    }
    if let Some(default) = &column.default {
        def.push_str(&format!(" DEFAULT {default}"));
    }
    def
}

fn emit_altered_table(
    dialect: &dyn SqlDialect,
    style: AlterColumnStyle,
    table: &AlteredTable,
    out: &mut Vec<MigrationStatement>,
    unsupported: &mut Vec<UnsupportedMigration>,
) {
    if let Some(reason) = unsupported_style_reason(dialect, style) {
        unsupported.push(UnsupportedMigration {
            operation: format!("alter table {}", table.name),
            reason,
        });
        return;
    }

    let quoted_table = dialect.quote_identifier(&table.name);
    if let Some(from) = &table.renamed_from {
        out.push(MigrationStatement {
            sql: rename_table_sql(dialect, style, from, &table.name),
            destructive: false,
        });
    }

    // Order so the script applies cleanly: add and alter columns, drop the
    // indexes/constraints that may reference soon-to-be-dropped columns, drop
    // columns, then (re)create constraints and indexes against the final column set.
    for column in &table.added_columns {
        out.push(MigrationStatement {
            sql: format!(
                "ALTER TABLE {quoted_table} ADD COLUMN {};",
                column_definition(dialect, column)
            ),
            destructive: false,
        });
    }
    for rename in &table.renamed_columns {
        out.push(MigrationStatement {
            sql: rename_column_sql(dialect, style, &quoted_table, rename),
            destructive: false,
        });
    }
    for altered in &table.altered_columns {
        match style {
            AlterColumnStyle::MySql => {
                out.push(MigrationStatement {
                    sql: format!(
                        "ALTER TABLE {quoted_table} MODIFY COLUMN {};",
                        column_definition(dialect, &altered.column)
                    ),
                    destructive: altered.changes.iter().any(ColumnChange::is_destructive),
                });
            }
            AlterColumnStyle::Standard => {
                let column = dialect.quote_identifier(&altered.column.name);
                for change in &altered.changes {
                    let sql = match change {
                        ColumnChange::Type { to, .. } => {
                            format!("ALTER TABLE {quoted_table} ALTER COLUMN {column} TYPE {to};")
                        }
                        ColumnChange::Nullability { nullable: false } => format!(
                            "ALTER TABLE {quoted_table} ALTER COLUMN {column} SET NOT NULL;"
                        ),
                        ColumnChange::Nullability { nullable: true } => format!(
                            "ALTER TABLE {quoted_table} ALTER COLUMN {column} DROP NOT NULL;"
                        ),
                        ColumnChange::Default {
                            to: Some(value), ..
                        } => format!(
                            "ALTER TABLE {quoted_table} ALTER COLUMN {column} SET DEFAULT {value};"
                        ),
                        ColumnChange::Default { to: None, .. } => format!(
                            "ALTER TABLE {quoted_table} ALTER COLUMN {column} DROP DEFAULT;"
                        ),
                    };
                    out.push(MigrationStatement {
                        sql,
                        destructive: change.is_destructive(),
                    });
                }
            }
        }
    }
    if let Some(change) = &table.primary_key_change {
        if !change.from.is_empty() {
            out.push(MigrationStatement {
                sql: drop_primary_key_sql(dialect, style, &table.name, &quoted_table),
                destructive: true,
            });
        }
        if !change.to.is_empty() {
            out.push(MigrationStatement {
                sql: add_primary_key_sql(dialect, &quoted_table, &change.to),
                destructive: false,
            });
        }
    }
    for rename in &table.renamed_constraints {
        match rename_constraint_sql(dialect, style, &quoted_table, rename) {
            Some(sql) => out.push(MigrationStatement {
                sql,
                destructive: false,
            }),
            None => unsupported.push(UnsupportedMigration {
                operation: format!("rename constraint {}.{}", table.name, rename.from),
                reason: format!("{style:?} does not support portable constraint rename SQL"),
            }),
        }
    }
    for constraint in &table.dropped_constraints {
        out.push(MigrationStatement {
            sql: drop_constraint_sql(dialect, style, &quoted_table, constraint),
            destructive: true,
        });
    }
    for name in &table.dropped_indexes {
        let sql = match style {
            // MySQL scopes a dropped index to its table.
            AlterColumnStyle::MySql => format!(
                "DROP INDEX {} ON {quoted_table};",
                dialect.quote_identifier(name)
            ),
            AlterColumnStyle::Standard => {
                format!("DROP INDEX {};", dialect.quote_identifier(name))
            }
        };
        out.push(MigrationStatement {
            sql,
            destructive: true,
        });
    }
    for rename in &table.renamed_indexes {
        out.push(MigrationStatement {
            sql: rename_index_sql(dialect, style, &quoted_table, rename),
            destructive: false,
        });
    }
    for name in &table.dropped_columns {
        out.push(MigrationStatement {
            sql: format!(
                "ALTER TABLE {quoted_table} DROP COLUMN {};",
                dialect.quote_identifier(name)
            ),
            destructive: true,
        });
    }
    for constraint in &table.added_constraints {
        out.push(MigrationStatement {
            sql: add_constraint_sql(dialect, &quoted_table, constraint),
            destructive: false,
        });
    }
    for index in &table.added_indexes {
        out.push(MigrationStatement {
            sql: create_index_sql(dialect, &table.name, index),
            destructive: false,
        });
    }
}

fn create_index_sql(dialect: &dyn SqlDialect, table: &str, index: &Index) -> String {
    let columns = index
        .columns
        .iter()
        .map(|column| dialect.quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE {}INDEX {} ON {} ({columns});",
        if index.unique { "UNIQUE " } else { "" },
        dialect.quote_identifier(&index.name),
        dialect.quote_identifier(table)
    )
}

fn unsupported_style_reason(dialect: &dyn SqlDialect, style: AlterColumnStyle) -> Option<String> {
    let placeholder = dialect.placeholder(1);
    let probe = dialect.quote_identifier("irodori_probe");
    match style {
        AlterColumnStyle::Standard if placeholder == "$1" && probe == "\"irodori_probe\"" => None,
        AlterColumnStyle::MySql if placeholder == "?" && probe == "`irodori_probe`" => None,
        _ => Some(format!(
            "{style:?} alter-table rendering is not supported for dialect hint placeholder={placeholder:?}, quoted_identifier={probe:?}"
        )),
    }
}

fn rename_table_sql(
    dialect: &dyn SqlDialect,
    style: AlterColumnStyle,
    from: &str,
    to: &str,
) -> String {
    match style {
        AlterColumnStyle::MySql => format!(
            "RENAME TABLE {} TO {};",
            dialect.quote_identifier(from),
            dialect.quote_identifier(to)
        ),
        AlterColumnStyle::Standard => format!(
            "ALTER TABLE {} RENAME TO {};",
            dialect.quote_identifier(from),
            dialect.quote_identifier(to)
        ),
    }
}

fn rename_column_sql(
    dialect: &dyn SqlDialect,
    style: AlterColumnStyle,
    quoted_table: &str,
    rename: &Rename,
) -> String {
    match style {
        AlterColumnStyle::MySql => format!(
            "ALTER TABLE {quoted_table} RENAME COLUMN {} TO {};",
            dialect.quote_identifier(&rename.from),
            dialect.quote_identifier(&rename.to)
        ),
        AlterColumnStyle::Standard => format!(
            "ALTER TABLE {quoted_table} RENAME COLUMN {} TO {};",
            dialect.quote_identifier(&rename.from),
            dialect.quote_identifier(&rename.to)
        ),
    }
}

fn rename_index_sql(
    dialect: &dyn SqlDialect,
    style: AlterColumnStyle,
    quoted_table: &str,
    rename: &Rename,
) -> String {
    match style {
        AlterColumnStyle::MySql => format!(
            "ALTER TABLE {quoted_table} RENAME INDEX {} TO {};",
            dialect.quote_identifier(&rename.from),
            dialect.quote_identifier(&rename.to)
        ),
        AlterColumnStyle::Standard => format!(
            "ALTER INDEX {} RENAME TO {};",
            dialect.quote_identifier(&rename.from),
            dialect.quote_identifier(&rename.to)
        ),
    }
}

fn rename_constraint_sql(
    dialect: &dyn SqlDialect,
    style: AlterColumnStyle,
    quoted_table: &str,
    rename: &Rename,
) -> Option<String> {
    match style {
        AlterColumnStyle::Standard => Some(format!(
            "ALTER TABLE {quoted_table} RENAME CONSTRAINT {} TO {};",
            dialect.quote_identifier(&rename.from),
            dialect.quote_identifier(&rename.to)
        )),
        AlterColumnStyle::MySql => None,
    }
}

fn drop_primary_key_sql(
    dialect: &dyn SqlDialect,
    style: AlterColumnStyle,
    table: &str,
    quoted_table: &str,
) -> String {
    match style {
        AlterColumnStyle::MySql => format!("ALTER TABLE {quoted_table} DROP PRIMARY KEY;"),
        AlterColumnStyle::Standard => format!(
            "ALTER TABLE {quoted_table} DROP CONSTRAINT {};",
            dialect.quote_identifier(&format!("{table}_pkey"))
        ),
    }
}

fn add_primary_key_sql(
    dialect: &dyn SqlDialect,
    quoted_table: &str,
    columns: &[String],
) -> String {
    let columns = columns
        .iter()
        .map(|column| dialect.quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    format!("ALTER TABLE {quoted_table} ADD PRIMARY KEY ({columns});")
}

fn add_constraint_sql(
    dialect: &dyn SqlDialect,
    quoted_table: &str,
    constraint: &TableConstraint,
) -> String {
    format!(
        "ALTER TABLE {quoted_table} ADD {};",
        constraint_definition(dialect, constraint)
    )
}

fn drop_constraint_sql(
    dialect: &dyn SqlDialect,
    style: AlterColumnStyle,
    quoted_table: &str,
    constraint: &TableConstraint,
) -> String {
    match (style, constraint) {
        (AlterColumnStyle::MySql, TableConstraint::ForeignKey(constraint)) => format!(
            "ALTER TABLE {quoted_table} DROP FOREIGN KEY {};",
            dialect.quote_identifier(&constraint.name)
        ),
        (AlterColumnStyle::MySql, TableConstraint::Check(constraint)) => format!(
            "ALTER TABLE {quoted_table} DROP CHECK {};",
            dialect.quote_identifier(&constraint.name)
        ),
        (AlterColumnStyle::MySql, TableConstraint::Unique(constraint)) => format!(
            "ALTER TABLE {quoted_table} DROP INDEX {};",
            dialect.quote_identifier(&constraint.name)
        ),
        (_, constraint) => format!(
            "ALTER TABLE {quoted_table} DROP CONSTRAINT {};",
            dialect.quote_identifier(constraint.name())
        ),
    }
}

fn constraint_definition(dialect: &dyn SqlDialect, constraint: &TableConstraint) -> String {
    match constraint {
        TableConstraint::ForeignKey(constraint) => {
            let columns = quote_list(dialect, &constraint.columns);
            let referenced_columns = quote_list(dialect, &constraint.referenced_columns);
            let mut sql = format!(
                "CONSTRAINT {} FOREIGN KEY ({columns}) REFERENCES {} ({referenced_columns})",
                dialect.quote_identifier(&constraint.name),
                dialect.quote_identifier(&constraint.referenced_table)
            );
            if let Some(action) = &constraint.on_delete {
                sql.push_str(&format!(" ON DELETE {action}"));
            }
            if let Some(action) = &constraint.on_update {
                sql.push_str(&format!(" ON UPDATE {action}"));
            }
            sql
        }
        TableConstraint::Check(constraint) => format!(
            "CONSTRAINT {} CHECK ({})",
            dialect.quote_identifier(&constraint.name),
            constraint.expression
        ),
        TableConstraint::Unique(constraint) => format!(
            "CONSTRAINT {} UNIQUE ({})",
            dialect.quote_identifier(&constraint.name),
            quote_list(dialect, &constraint.columns)
        ),
    }
}

fn quote_list(dialect: &dyn SqlDialect, names: &[String]) -> String {
    names
        .iter()
        .map(|name| dialect.quote_identifier(name))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{MySqlDialect, PostgresDialect};

    fn col(name: &str, ty: &str) -> Column {
        Column::new(name, ty)
    }

    fn users_v1() -> Table {
        Table::new("users")
            .with_columns(vec![
                col("id", "integer").not_null(),
                col("email", "text").not_null(),
                col("note", "text"),
            ])
            .with_primary_key(vec!["id".into()])
            .with_indexes(vec![Index {
                name: "users_email_idx".into(),
                columns: vec!["email".into()],
                unique: true,
            }])
    }

    #[test]
    fn identical_schemas_have_no_diff() {
        let schema = Schema::new(vec![users_v1()]);
        let diff = diff_schemas(&schema, &schema);
        assert!(diff.is_empty());
        assert!(!diff.has_destructive_changes());
        assert_eq!(diff.summary(), "no schema changes");
        assert!(diff
            .to_migration(&PostgresDialect, AlterColumnStyle::Standard)
            .is_empty());
    }

    #[test]
    fn added_table_creates_with_columns_and_primary_key() {
        let old = Schema::default();
        let new = Schema::new(vec![users_v1()]);
        let diff = diff_schemas(&old, &new);
        assert_eq!(diff.added_tables.len(), 1);
        assert!(!diff.has_destructive_changes());

        let script = diff.to_migration(&PostgresDialect, AlterColumnStyle::Standard);
        assert_eq!(script.statements.len(), 2); // create table + create index
        assert_eq!(
            script.statements[0].sql,
            "CREATE TABLE \"users\" (\n  \"id\" integer NOT NULL,\n  \"email\" text NOT NULL,\n  \"note\" text,\n  PRIMARY KEY (\"id\")\n);"
        );
        assert_eq!(
            script.statements[1].sql,
            "CREATE UNIQUE INDEX \"users_email_idx\" ON \"users\" (\"email\");"
        );
        assert_eq!(script.destructive_count(), 0);
    }

    #[test]
    fn dropped_table_is_destructive() {
        let old = Schema::new(vec![users_v1()]);
        let new = Schema::default();
        let diff = diff_schemas(&old, &new);
        assert_eq!(diff.dropped_tables, vec!["users".to_string()]);
        assert!(diff.has_destructive_changes());
        let script = diff.to_migration(&PostgresDialect, AlterColumnStyle::Standard);
        assert_eq!(script.statements[0].sql, "DROP TABLE \"users\";");
        assert!(script.statements[0].destructive);
    }

    #[test]
    fn column_add_drop_and_alter_postgres() {
        let old = Schema::new(vec![users_v1()]);
        // v2: drop `note`, add `active bool not null default true`, widen email
        // type, make it nullable.
        let new = Schema::new(vec![Table::new("users")
            .with_columns(vec![
                col("id", "integer").not_null(),
                col("email", "varchar(320)"),
                col("active", "boolean").not_null().with_default("true"),
            ])
            .with_primary_key(vec!["id".into()])
            .with_indexes(vec![Index {
                name: "users_email_idx".into(),
                columns: vec!["email".into()],
                unique: true,
            }])]);

        let diff = diff_schemas(&old, &new);
        let altered = &diff.altered_tables[0];
        assert_eq!(altered.name, "users");
        assert_eq!(altered.added_columns.len(), 1);
        assert_eq!(altered.dropped_columns, vec!["note".to_string()]);
        assert_eq!(altered.altered_columns.len(), 1); // email: type + nullability
        assert!(diff.has_destructive_changes()); // dropping `note`

        let sql = diff
            .to_migration(&PostgresDialect, AlterColumnStyle::Standard)
            .to_sql();
        assert!(sql.contains(
            "ALTER TABLE \"users\" ADD COLUMN \"active\" boolean NOT NULL DEFAULT true;"
        ));
        assert!(sql.contains("ALTER TABLE \"users\" DROP COLUMN \"note\";"));
        assert!(sql.contains("ALTER TABLE \"users\" ALTER COLUMN \"email\" TYPE varchar(320);"));
        assert!(sql.contains("ALTER TABLE \"users\" ALTER COLUMN \"email\" DROP NOT NULL;"));
    }

    #[test]
    fn alter_column_mysql_uses_modify_and_backticks() {
        let old = Schema::new(vec![
            Table::new("t").with_columns(vec![col("c", "int").not_null()])
        ]);
        let new = Schema::new(vec![
            Table::new("t").with_columns(vec![col("c", "bigint").with_default("0")])
        ]);
        let diff = diff_schemas(&old, &new);
        let script = diff.to_migration(&MySqlDialect, AlterColumnStyle::MySql);
        // One MODIFY statement carrying the full target definition.
        assert_eq!(script.statements.len(), 1);
        assert_eq!(
            script.statements[0].sql,
            "ALTER TABLE `t` MODIFY COLUMN `c` bigint DEFAULT 0;"
        );
    }

    #[test]
    fn index_add_drop_and_recreate_on_change() {
        let old = Schema::new(vec![Table::new("t").with_indexes(vec![
            Index {
                name: "keep".into(),
                columns: vec!["a".into()],
                unique: false,
            },
            Index {
                name: "gone".into(),
                columns: vec!["b".into()],
                unique: false,
            },
            Index {
                name: "changed".into(),
                columns: vec!["c".into()],
                unique: false,
            },
        ])]);
        let new = Schema::new(vec![Table::new("t").with_indexes(vec![
            Index {
                name: "keep".into(),
                columns: vec!["a".into()],
                unique: false,
            },
            Index {
                name: "changed".into(),
                columns: vec!["c".into(), "d".into()], // now multi-column
                unique: false,
            },
            Index {
                name: "fresh".into(),
                columns: vec!["e".into()],
                unique: true,
            },
        ])]);

        let diff = diff_schemas(&old, &new);
        let altered = &diff.altered_tables[0];
        // `gone` dropped; `changed` recreated (drop + add); `fresh` added.
        assert!(altered.dropped_indexes.contains(&"gone".to_string()));
        assert!(altered.dropped_indexes.contains(&"changed".to_string()));
        assert_eq!(altered.added_indexes.len(), 2); // changed + fresh

        let mysql = diff
            .to_migration(&MySqlDialect, AlterColumnStyle::MySql)
            .to_sql();
        assert!(mysql.contains("DROP INDEX `gone` ON `t`;"));
        assert!(mysql.contains("CREATE INDEX `changed` ON `t` (`c`, `d`);"));
        assert!(mysql.contains("CREATE UNIQUE INDEX `fresh` ON `t` (`e`);"));
    }

    #[test]
    fn summary_reads_clearly() {
        let old = Schema::new(vec![users_v1(), Table::new("temp")]);
        let new = Schema::new(vec![Table::new("users")
            .with_columns(vec![
                col("id", "integer").not_null(),
                col("created", "timestamp"),
            ])
            .with_primary_key(vec!["id".into()])
            .with_indexes(vec![Index {
                name: "users_email_idx".into(),
                columns: vec!["email".into()],
                unique: true,
            }])]);
        let diff = diff_schemas(&old, &new);
        let summary = diff.summary();
        assert!(summary.contains("-1 table(s)")); // temp dropped
        assert!(summary.contains("users"));
    }
}
