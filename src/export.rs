//! Standalone row export runner.
//!
//! The desktop app can wrap this with its own job runtime, but the core crate
//! keeps cancellation and progress as simple callbacks so it can be published
//! without depending on an application crate.

use std::io;

use crate::io::{Cell, OwnedCell, TabularEncoder};

const DEFAULT_PROGRESS_EVERY_ROWS: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportConfig {
    /// Report progress every this many rows. Cancellation is checked before each
    /// row so hosts can bridge an async cancellation token with an AtomicBool.
    pub progress_every_rows: u64,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            progress_every_rows: DEFAULT_PROGRESS_EVERY_ROWS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub rows_written: u64,
    pub cancelled: bool,
}

pub trait ExportControl {
    fn should_cancel(&mut self) -> bool {
        false
    }

    fn progress(&mut self, _rows_written: u64) -> io::Result<()> {
        Ok(())
    }
}

impl ExportControl for () {}

/// Stream rows through an encoder with cooperative cancellation and progress.
///
/// Cancellation is a normal successful outcome. The encoder is always flushed,
/// including after a partial export.
#[tracing::instrument(skip_all, fields(progress_every_rows = config.progress_every_rows))]
pub fn export_rows<R>(
    rows: R,
    encoder: &mut dyn TabularEncoder,
    control: &mut dyn ExportControl,
    config: ExportConfig,
) -> io::Result<ExportReport>
where
    R: IntoIterator<Item = Vec<OwnedCell>>,
{
    let progress_every = config.progress_every_rows.max(1);
    let mut rows_written = 0u64;
    let mut cancelled = false;

    for row in rows {
        if control.should_cancel() {
            cancelled = true;
            break;
        }
        let cells: Vec<Cell<'_>> = row.iter().map(cell_ref).collect();
        encoder.write_row(&cells)?;
        rows_written += 1;

        if rows_written.is_multiple_of(progress_every) {
            control.progress(rows_written)?;
        }
    }

    encoder.finish()?;
    if !cancelled {
        control.progress(rows_written)?;
    }

    tracing::debug!(rows_written, cancelled, "export finished");
    Ok(ExportReport {
        rows_written,
        cancelled,
    })
}

fn cell_ref(owned: &OwnedCell) -> Cell<'_> {
    match owned {
        OwnedCell::Null => Cell::Null,
        OwnedCell::Bool(value) => Cell::Bool(*value),
        OwnedCell::Integer(value) => Cell::Integer(*value),
        OwnedCell::Float(value) => Cell::Float(*value),
        OwnedCell::Text(value) => Cell::Text(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::DelimitedEncoder;

    #[derive(Default)]
    struct CancelAfterFirstProgress {
        calls: usize,
    }

    impl ExportControl for CancelAfterFirstProgress {
        fn should_cancel(&mut self) -> bool {
            self.calls >= 1
        }

        fn progress(&mut self, _rows_written: u64) -> io::Result<()> {
            self.calls += 1;
            Ok(())
        }
    }

    #[test]
    fn exports_rows_and_reports_progress() {
        let rows = vec![
            vec![OwnedCell::Integer(1), OwnedCell::Text("a".into())],
            vec![OwnedCell::Integer(2), OwnedCell::Text("b".into())],
        ];
        let mut out = Vec::new();
        let mut encoder = DelimitedEncoder::csv(&mut out, &["id", "name"]).unwrap();
        let mut control = ();

        let report = export_rows(
            rows,
            &mut encoder,
            &mut control,
            ExportConfig {
                progress_every_rows: 1,
            },
        )
        .unwrap();

        assert_eq!(report.rows_written, 2);
        assert!(!report.cancelled);
        assert_eq!(String::from_utf8(out).unwrap(), "id,name\n1,a\n2,b\n");
    }

    #[test]
    fn cancellation_flushes_partial_output() {
        let rows = (0..100).map(|value| vec![OwnedCell::Integer(value)]);
        let mut out = Vec::new();
        let mut encoder = DelimitedEncoder::csv(&mut out, &["n"]).unwrap();
        let mut control = CancelAfterFirstProgress::default();

        let report = export_rows(
            rows,
            &mut encoder,
            &mut control,
            ExportConfig {
                progress_every_rows: 10,
            },
        )
        .unwrap();

        assert!(report.cancelled);
        assert_eq!(report.rows_written, 10);
        assert!(String::from_utf8(out).unwrap().starts_with("n\n0\n"));
    }
}
