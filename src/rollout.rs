//! Zero-downtime rollout plans for database migrations.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutPhase {
    Expand,
    DualWrite,
    Backfill,
    ShadowRead,
    Canary,
    Cutover,
    Contract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutGate {
    pub name: String,
    pub check: String,
    pub rollback: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutStep {
    pub phase: RolloutPhase,
    pub title: String,
    pub detail: String,
    pub gates: Vec<RolloutGate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutPlan {
    pub title: String,
    pub steps: Vec<RolloutStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationValidationSummary {
    pub source_row_count: u64,
    pub target_row_count: u64,
    pub source_key_count: u64,
    pub target_key_count: u64,
    pub failed_chunk_count: u64,
    pub row_diff_count: u64,
    pub shadow_mismatch_count: u64,
}

impl MigrationValidationSummary {
    pub fn passes_checksum_gates(&self) -> bool {
        self.source_row_count == self.target_row_count
            && self.source_key_count == self.target_key_count
            && self.failed_chunk_count == 0
    }

    pub fn passes_cutover_gates(&self) -> bool {
        self.passes_checksum_gates() && self.row_diff_count == 0 && self.shadow_mismatch_count == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowReadExperiment {
    pub name: String,
    pub control_query: String,
    pub candidate_query: String,
    pub comparison_key: Vec<String>,
    pub sample_percent: u8,
}

impl ShadowReadExperiment {
    pub fn new(
        name: impl Into<String>,
        control_query: impl Into<String>,
        candidate_query: impl Into<String>,
        comparison_key: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            control_query: control_query.into(),
            candidate_query: candidate_query.into(),
            comparison_key,
            sample_percent: 1,
        }
    }

    pub fn with_sample_percent(mut self, sample_percent: u8) -> Self {
        self.sample_percent = sample_percent.clamp(1, 100);
        self
    }
}

pub fn expand_contract_rollout(title: impl Into<String>) -> RolloutPlan {
    RolloutPlan {
        title: title.into(),
        steps: vec![
            RolloutStep {
                phase: RolloutPhase::Expand,
                title: "Expand".to_string(),
                detail: "Apply backward-compatible DDL: add nullable columns, new tables, indexes created online, and compatibility views.".to_string(),
                gates: vec![gate("DDL preview approved", "destructive_count = 0", "revert new compatibility objects")],
            },
            RolloutStep {
                phase: RolloutPhase::DualWrite,
                title: "Dual write".to_string(),
                detail: "Write old and new shapes behind a feature flag while reads still use the old path.".to_string(),
                gates: vec![gate("Write parity", "control_write_errors = candidate_write_errors", "disable candidate writes")],
            },
            RolloutStep {
                phase: RolloutPhase::Backfill,
                title: "Backfill".to_string(),
                detail: "Copy historical data in throttled, resumable batches with row-count and checksum gates.".to_string(),
                gates: vec![gate("Lag budget", "replica_lag_seconds <= max_lag_seconds", "pause backfill worker")],
            },
            RolloutStep {
                phase: RolloutPhase::ShadowRead,
                title: "Shadow read".to_string(),
                detail: "Read from both old and new paths, return control results, and record mismatches.".to_string(),
                gates: vec![gate("Read parity", "mismatch_rate = 0", "keep control read path")],
            },
            RolloutStep {
                phase: RolloutPhase::Canary,
                title: "Canary".to_string(),
                detail: "Serve a small percentage from the candidate path and watch correctness and latency.".to_string(),
                gates: vec![gate("Canary health", "error_budget_ok AND latency_budget_ok", "route traffic back to control")],
            },
            RolloutStep {
                phase: RolloutPhase::Cutover,
                title: "Cutover".to_string(),
                detail: "Switch primary reads and writes to the candidate after checksums and shadow reads pass.".to_string(),
                gates: vec![gate("Final diff", "failed_chunks = 0 AND row_diff_count = 0", "switch feature flag back")],
            },
            RolloutStep {
                phase: RolloutPhase::Contract,
                title: "Contract".to_string(),
                detail: "Remove old columns, tables, and compatibility code only after rollback windows expire.".to_string(),
                gates: vec![gate("Rollback window elapsed", "rollback_window_closed = true", "keep old schema objects")],
            },
        ],
    }
}

pub fn validation_rollout_gates(summary: &MigrationValidationSummary) -> Vec<RolloutGate> {
    vec![
        gate(
            "Row count parity",
            &format!(
                "source_row_count = target_row_count ({} = {})",
                summary.source_row_count, summary.target_row_count
            ),
            "pause workers and keep dual-write enabled",
        ),
        gate(
            "Key count parity",
            &format!(
                "source_key_count = target_key_count ({} = {})",
                summary.source_key_count, summary.target_key_count
            ),
            "inspect duplicate or missing business keys before continuing",
        ),
        gate(
            "Chunk checksum parity",
            &format!("failed_chunk_count = 0 ({})", summary.failed_chunk_count),
            "recurse into failed chunks and keep candidate reads disabled",
        ),
        gate(
            "Final row diff",
            &format!("row_diff_count = 0 ({})", summary.row_diff_count),
            "switch feature flag back and repair only mismatched rows",
        ),
        gate(
            "Shadow read parity",
            &format!(
                "shadow_mismatch_count = 0 ({})",
                summary.shadow_mismatch_count
            ),
            "keep control read path",
        ),
    ]
}

pub fn attach_validation_gates(
    mut plan: RolloutPlan,
    summary: &MigrationValidationSummary,
) -> RolloutPlan {
    let gates = validation_rollout_gates(summary);
    for step in &mut plan.steps {
        match step.phase {
            RolloutPhase::Backfill => {
                step.gates.extend(gates.iter().take(3).cloned());
            }
            RolloutPhase::Cutover => {
                step.gates.extend(gates.iter().skip(2).cloned());
            }
            RolloutPhase::ShadowRead => {
                if let Some(gate) = gates.iter().find(|gate| gate.name == "Shadow read parity") {
                    step.gates.push(gate.clone());
                }
            }
            _ => {}
        }
    }
    plan
}

pub fn shadow_read_runbook(experiment: &ShadowReadExperiment) -> String {
    [
        format!("# Shadow Read: {}", experiment.name),
        format!("- Sample: {}%", experiment.sample_percent),
        format!("- Key: {}", experiment.comparison_key.join(", ")),
        "- Return control query results to users.".to_string(),
        "- Execute candidate query out-of-band and compare canonicalized values.".to_string(),
        "- Publish mismatches with control, candidate, key, and normalized diff evidence."
            .to_string(),
        String::new(),
        "## Control".to_string(),
        experiment.control_query.clone(),
        String::new(),
        "## Candidate".to_string(),
        experiment.candidate_query.clone(),
    ]
    .join("\n")
}

fn gate(name: &str, check: &str, rollback: &str) -> RolloutGate {
    RolloutGate {
        name: name.to_string(),
        check: check.to_string(),
        rollback: rollback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_contract_plan_has_ordered_safety_phases() {
        let plan = expand_contract_rollout("orders migration");

        assert_eq!(plan.steps.first().unwrap().phase, RolloutPhase::Expand);
        assert_eq!(plan.steps.last().unwrap().phase, RolloutPhase::Contract);
        assert!(plan
            .steps
            .iter()
            .any(|step| step.phase == RolloutPhase::ShadowRead));
    }

    #[test]
    fn shadow_read_runbook_keeps_control_and_candidate_separate() {
        let runbook = shadow_read_runbook(
            &ShadowReadExperiment::new(
                "orders read",
                "select * from old_orders",
                "select * from new_orders",
                vec!["order_id".to_string()],
            )
            .with_sample_percent(5),
        );

        assert!(runbook.contains("Return control query results"));
        assert!(runbook.contains("select * from old_orders"));
        assert!(runbook.contains("select * from new_orders"));
    }

    #[test]
    fn validation_summary_attaches_checksum_and_diff_gates() {
        let summary = MigrationValidationSummary {
            source_row_count: 100,
            target_row_count: 100,
            source_key_count: 100,
            target_key_count: 100,
            failed_chunk_count: 0,
            row_diff_count: 0,
            shadow_mismatch_count: 0,
        };

        let plan = attach_validation_gates(expand_contract_rollout("orders"), &summary);
        let backfill = plan
            .steps
            .iter()
            .find(|step| step.phase == RolloutPhase::Backfill)
            .expect("backfill step");
        let cutover = plan
            .steps
            .iter()
            .find(|step| step.phase == RolloutPhase::Cutover)
            .expect("cutover step");

        assert!(summary.passes_cutover_gates());
        assert!(backfill
            .gates
            .iter()
            .any(|gate| gate.name == "Chunk checksum parity"));
        assert!(cutover
            .gates
            .iter()
            .any(|gate| gate.check.contains("row_diff_count = 0")));
    }
}
