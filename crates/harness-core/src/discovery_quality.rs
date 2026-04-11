use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::discovery::{
    CommandCatalog, DiscoveryEvidenceChainStrength, DiscoveryFact, NegentropyTier,
    WorkspaceDiscoveryEvidence, WorkspaceDiscoveryInference, WorkspaceDiscoveryPhase,
    WorkspaceDiscoveryRequest, WorkspaceDiscoveryStatus, WorkspaceDiscoveryStore, WorkspaceProfile,
};

const DISCOVERY_QUALITY_RUBRIC_VERSION: &str = "2026-04-11.negentropy-v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryQualityGate {
    Pass,
    Warn,
    Fail,
}

impl DiscoveryQualityGate {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryQualitySeverity {
    Info,
    Warning,
    Error,
}

impl DiscoveryQualitySeverity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryQualityDimension {
    pub name: String,
    pub score: u8,
    pub gate: DiscoveryQualityGate,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryQualityFinding {
    pub severity: DiscoveryQualitySeverity,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub related_paths: Vec<PathBuf>,
    #[serde(default)]
    pub related_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryQualityMetrics {
    pub evidence_item_count: usize,
    pub inference_count: usize,
    pub profile_key_concept_count: usize,
    pub specification_fact_count: usize,
    pub verification_fact_count: usize,
    pub structure_fact_count: usize,
    pub implementation_fact_count: usize,
    pub missing_provenance_count: usize,
    pub duplicate_display_item_count: usize,
    pub unsupported_profile_claim_count: usize,
    pub heuristic_layer_rule_count: usize,
    pub confidence_ten_policy_violations: usize,
    pub profile_faithfulness_mismatch_count: usize,
    pub stale_artifacts: bool,
    pub used_fallback_profile: bool,
    pub missing_artifact_count: usize,
    pub current_phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryQualityReport {
    pub rubric_version: String,
    pub workspace_path: PathBuf,
    pub evaluated_at: DateTime<Utc>,
    pub gate: DiscoveryQualityGate,
    pub overall_score: u8,
    pub summary: String,
    pub dimensions: Vec<DiscoveryQualityDimension>,
    pub metrics: DiscoveryQualityMetrics,
    #[serde(default)]
    pub findings: Vec<DiscoveryQualityFinding>,
    #[serde(default)]
    pub recommendations: Vec<String>,
}

pub fn discovery_quality_report_path(store: &WorkspaceDiscoveryStore) -> PathBuf {
    store.root().join("quality-report.json")
}

pub fn save_discovery_quality_report(
    store: &WorkspaceDiscoveryStore,
    report: &DiscoveryQualityReport,
) -> Result<()> {
    store.ensure_dirs()?;
    let path = discovery_quality_report_path(store);
    let bytes = serde_json::to_vec_pretty(report).context("failed to serialize quality report")?;
    fs::write(&path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

pub fn evaluate_discovery_store(store: &WorkspaceDiscoveryStore) -> Result<DiscoveryQualityReport> {
    let status = store.load_status()?;
    let evidence = store.load_evidence()?;
    let inference = store.load_inference()?;
    let profile = store.load_profile()?;
    evaluate_discovery_artifacts(
        status.as_ref(),
        evidence.as_ref(),
        inference.as_ref(),
        profile.as_ref(),
    )
}

pub fn evaluate_discovery_artifacts(
    status: Option<&WorkspaceDiscoveryStatus>,
    evidence: Option<&WorkspaceDiscoveryEvidence>,
    inference: Option<&WorkspaceDiscoveryInference>,
    profile: Option<&WorkspaceProfile>,
) -> Result<DiscoveryQualityReport> {
    let workspace_path = status
        .map(|item| item.workspace_path.clone())
        .or_else(|| evidence.map(|item| item.workspace_path.clone()))
        .or_else(|| inference.map(|item| item.workspace_path.clone()))
        .or_else(|| profile.map(|item| item.workspace_path.clone()))
        .context("no discovery artifacts found to evaluate")?;

    let missing_artifact_count = usize::from(status.is_none())
        + usize::from(evidence.is_none())
        + usize::from(inference.is_none())
        + usize::from(profile.is_none());

    let stale_artifacts = status.is_some_and(|item| {
        item.last_refreshed_at
            .map(|refreshed_at| item.last_scanned_at > refreshed_at)
            .unwrap_or(true)
            || item.current_phase != WorkspaceDiscoveryPhase::Ready
    });
    let used_fallback_profile = status.is_some_and(|item| item.used_fallback_profile);
    let current_phase = status
        .map(|item| item.current_phase.as_str().to_string())
        .unwrap_or_else(|| "missing".to_string());

    let fact_counts = evidence
        .map(collect_tier_counts)
        .unwrap_or_else(BTreeMap::new);
    let evidence_item_count = evidence.map_or(0, total_evidence_item_count);
    let inference_count = inference.map_or(0, |item| item.inferences.len());
    let profile_key_concept_count = profile.map_or(0, |item| item.key_concepts.len());
    let missing_provenance_count = evidence.map_or(0, count_missing_provenance);
    let duplicate_display_item_count = evidence.map_or(0, count_duplicate_display_items);
    let confidence_ten_policy_violations =
        inference.map_or(0, count_confidence_ten_policy_violations);
    let heuristic_layer_rule_count =
        count_heuristic_layer_rule_violations(evidence, inference, profile);
    let unsupported_profile_claim_count =
        count_unsupported_profile_claims(evidence, inference, profile);
    let profile_faithfulness_mismatch_count =
        count_profile_faithfulness_mismatches(evidence, inference, profile);

    let metrics = DiscoveryQualityMetrics {
        evidence_item_count,
        inference_count,
        profile_key_concept_count,
        specification_fact_count: tier_count(&fact_counts, NegentropyTier::Specification),
        verification_fact_count: tier_count(&fact_counts, NegentropyTier::Verification),
        structure_fact_count: tier_count(&fact_counts, NegentropyTier::Structure),
        implementation_fact_count: tier_count(&fact_counts, NegentropyTier::Implementation),
        missing_provenance_count,
        duplicate_display_item_count,
        unsupported_profile_claim_count,
        heuristic_layer_rule_count,
        confidence_ten_policy_violations,
        profile_faithfulness_mismatch_count,
        stale_artifacts,
        used_fallback_profile,
        missing_artifact_count,
        current_phase,
    };

    let mut findings = Vec::new();
    if missing_artifact_count > 0 {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Error,
            code: "missing_artifacts".to_string(),
            message: format!(
                "Discovery quality evaluation is missing {} required artifact set(s).",
                missing_artifact_count
            ),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if stale_artifacts {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Error,
            code: "stale_or_incomplete_discovery".to_string(),
            message:
                "Discovery artifacts are stale or still in progress; planners should not treat them as current governance data."
                    .to_string(),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if used_fallback_profile {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Error,
            code: "fallback_profile_in_use".to_string(),
            message:
                "Discovery is serving a fallback profile rather than a fresh workspace-aligned result."
                    .to_string(),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if let (Some(evidence), Some(inference)) = (evidence, inference) {
        if let Err(err) = inference.validate(evidence) {
            findings.push(DiscoveryQualityFinding {
                severity: DiscoveryQualitySeverity::Error,
                code: "invalid_inference_structure".to_string(),
                message: err.to_string(),
                related_paths: Vec::new(),
                related_ids: Vec::new(),
            });
        }
    }
    if confidence_ten_policy_violations > 0 {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Error,
            code: "confidence_ten_policy_violation".to_string(),
            message: format!(
                "{} inference(s) used confidence 10 without any strong evidence chain.",
                confidence_ten_policy_violations
            ),
            related_paths: Vec::new(),
            related_ids: inference
                .map(|item| {
                    item.inferences
                        .iter()
                        .filter(|inference| {
                            inference.confidence == 10
                                && !inference.evidence_chains.iter().any(|chain| {
                                    chain.strength == DiscoveryEvidenceChainStrength::Strong
                                })
                        })
                        .map(|item| item.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    if heuristic_layer_rule_count > 0 {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Error,
            code: "heuristic_layer_rule_promotion".to_string(),
            message: format!(
                "{} layer rule claim(s) outran explicit architecture evidence.",
                heuristic_layer_rule_count
            ),
            related_paths: evidence
                .map(|item| {
                    item.layering
                        .layers
                        .iter()
                        .flat_map(|layer| layer.paths.clone())
                        .take(6)
                        .collect()
                })
                .unwrap_or_default(),
            related_ids: inference
                .map(|item| {
                    item.inferences
                        .iter()
                        .filter(|entry| entry.category == "layering_rule")
                        .map(|entry| entry.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    if unsupported_profile_claim_count > 0 {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Error,
            code: "unsupported_profile_claim".to_string(),
            message: format!(
                "{} profile claim(s) were not supported by evidence or inference.",
                unsupported_profile_claim_count
            ),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if profile_faithfulness_mismatch_count > 0 {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Error,
            code: "profile_faithfulness_mismatch".to_string(),
            message:
                "The stored profile does not match the profile deterministically assembled from evidence and inference."
                    .to_string(),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if missing_provenance_count > 0 {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Warning,
            code: "missing_provenance".to_string(),
            message: format!(
                "{} evidence item(s) were missing provenance paths or sources.",
                missing_provenance_count
            ),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if duplicate_display_item_count > 0 {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Warning,
            code: "duplicate_display_values".to_string(),
            message: format!(
                "{} duplicate display value(s) remain in raw discovery categories.",
                duplicate_display_item_count
            ),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if metrics.specification_fact_count + metrics.verification_fact_count == 0
        && evidence_item_count > 0
    {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Warning,
            code: "low_governance_density".to_string(),
            message:
                "Discovery found structural or implementation facts, but no specification- or verification-tier governance signals."
                    .to_string(),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }
    if evidence.is_some_and(|item| item.project_intent.is_empty()) {
        findings.push(DiscoveryQualityFinding {
            severity: DiscoveryQualitySeverity::Warning,
            code: "missing_project_intent".to_string(),
            message:
                "No project intent facts were discovered; planners may fall back to code-local context instead of goal-level constraints."
                    .to_string(),
            related_paths: Vec::new(),
            related_ids: Vec::new(),
        });
    }

    let dimensions = build_dimensions(&metrics, &findings);
    let overall_score = average_dimension_score(&dimensions);
    let gate = gate_from_findings_and_score(&findings, overall_score);
    let recommendations = build_recommendations(&findings);
    let summary = format!(
        "Discovery quality is {} with overall score {} under rubric {}.",
        gate.as_str(),
        overall_score,
        DISCOVERY_QUALITY_RUBRIC_VERSION
    );

    Ok(DiscoveryQualityReport {
        rubric_version: DISCOVERY_QUALITY_RUBRIC_VERSION.to_string(),
        workspace_path,
        evaluated_at: Utc::now(),
        gate,
        overall_score,
        summary,
        dimensions,
        metrics,
        findings,
        recommendations,
    })
}

fn build_dimensions(
    metrics: &DiscoveryQualityMetrics,
    findings: &[DiscoveryQualityFinding],
) -> Vec<DiscoveryQualityDimension> {
    let evidence_fidelity = clamp_score(
        100 - (metrics.missing_provenance_count as i32 * 10).min(40)
            - (metrics.duplicate_display_item_count as i32 * 5).min(20)
            - if metrics.missing_artifact_count > 0 {
                30
            } else {
                0
            },
    );
    let governance_fidelity = clamp_score(
        100 - (metrics.unsupported_profile_claim_count as i32 * 25).min(75)
            - (metrics.heuristic_layer_rule_count as i32 * 20).min(60)
            - if findings
                .iter()
                .any(|item| item.code == "low_governance_density")
            {
                20
            } else {
                0
            }
            - if findings
                .iter()
                .any(|item| item.code == "missing_project_intent")
            {
                10
            } else {
                0
            },
    );
    let compression_faithfulness = clamp_score(
        100 - (metrics.profile_faithfulness_mismatch_count as i32 * 40).min(80)
            - (metrics.duplicate_display_item_count as i32 * 8).min(24)
            - (metrics.unsupported_profile_claim_count as i32 * 12).min(36),
    );
    let operational_readiness = clamp_score(
        100 - if metrics.stale_artifacts { 45 } else { 0 }
            - if metrics.used_fallback_profile { 35 } else { 0 }
            - if metrics.current_phase != WorkspaceDiscoveryPhase::Ready.as_str() {
                25
            } else {
                0
            }
            - if metrics.inference_count == 0 { 25 } else { 0 }
            - if metrics.missing_artifact_count > 0 {
                25
            } else {
                0
            },
    );

    vec![
        dimension(
            "source_fidelity",
            evidence_fidelity,
            "Provenance completeness, raw evidence precision, and duplicate suppression.",
        ),
        dimension(
            "governance_fidelity",
            governance_fidelity,
            "Whether higher-negentropy constraints govern lower-negentropy observations without inversion.",
        ),
        dimension(
            "compression_faithfulness",
            compression_faithfulness,
            "Whether the stored profile remains a faithful compression of evidence and inference.",
        ),
        dimension(
            "operational_readiness",
            operational_readiness,
            "Whether the saved discovery artifacts are fresh and safe to inject into planner/build prompts.",
        ),
    ]
}

fn dimension(name: &str, score: u8, summary: &str) -> DiscoveryQualityDimension {
    DiscoveryQualityDimension {
        name: name.to_string(),
        score,
        gate: gate_from_score(score),
        summary: summary.to_string(),
    }
}

fn gate_from_score(score: u8) -> DiscoveryQualityGate {
    if score >= 85 {
        DiscoveryQualityGate::Pass
    } else if score >= 65 {
        DiscoveryQualityGate::Warn
    } else {
        DiscoveryQualityGate::Fail
    }
}

fn gate_from_findings_and_score(
    findings: &[DiscoveryQualityFinding],
    overall_score: u8,
) -> DiscoveryQualityGate {
    if findings
        .iter()
        .any(|item| item.severity == DiscoveryQualitySeverity::Error)
        || overall_score < 60
    {
        DiscoveryQualityGate::Fail
    } else if findings
        .iter()
        .any(|item| item.severity == DiscoveryQualitySeverity::Warning)
        || overall_score < 80
    {
        DiscoveryQualityGate::Warn
    } else {
        DiscoveryQualityGate::Pass
    }
}

fn average_dimension_score(dimensions: &[DiscoveryQualityDimension]) -> u8 {
    if dimensions.is_empty() {
        return 0;
    }
    let total: usize = dimensions.iter().map(|item| usize::from(item.score)).sum();
    (total / dimensions.len()) as u8
}

fn clamp_score(value: i32) -> u8 {
    value.clamp(0, 100) as u8
}

fn build_recommendations(findings: &[DiscoveryQualityFinding]) -> Vec<String> {
    let mut recommendations = Vec::new();
    let mut seen = BTreeSet::new();
    for finding in findings {
        let recommendation = match finding.code.as_str() {
            "stale_or_incomplete_discovery" | "fallback_profile_in_use" => {
                "Refresh discovery to a terminal ready state before using the profile as governance context."
            }
            "heuristic_layer_rule_promotion" | "unsupported_profile_claim" => {
                "Downgrade unsupported claims to ambiguities or add explicit evidence before allowing them into the profile."
            }
            "confidence_ten_policy_violation" => {
                "Restrict confidence 10 to inferences with at least one strong evidence chain and downgrade the rest."
            }
            "low_governance_density" | "missing_project_intent" => {
                "Increase discovery coverage for goals, specs, contracts, and E2E signals so high-negentropy artifacts govern code-level context."
            }
            "profile_faithfulness_mismatch" => {
                "Rebuild the profile from evidence and inference and block save paths that introduce untraceable claims."
            }
            "missing_provenance" => {
                "Ensure every discovered fact carries at least one concrete evidence path or source reference."
            }
            "duplicate_display_values" => {
                "Deduplicate rendered fact and command values before summarization so token budget is not wasted on repeated context."
            }
            _ => continue,
        };
        if seen.insert(recommendation) {
            recommendations.push(recommendation.to_string());
        }
    }
    recommendations
}

fn total_evidence_item_count(evidence: &WorkspaceDiscoveryEvidence) -> usize {
    evidence.source_files.len()
        + all_fact_refs(evidence).len()
        + evidence.repositories.len()
        + evidence.dependency_relationships.len()
        + total_command_count(&evidence.commands)
}

fn total_command_count(commands: &CommandCatalog) -> usize {
    commands.build.len() + commands.test.len() + commands.dev.len()
}

fn count_missing_provenance(evidence: &WorkspaceDiscoveryEvidence) -> usize {
    let fact_missing = all_fact_refs(evidence)
        .into_iter()
        .filter(|fact| fact.evidence.is_empty())
        .count();
    let repo_missing = evidence
        .repositories
        .iter()
        .filter(|repo| repo.evidence.is_empty())
        .count();
    let relationship_missing = evidence
        .dependency_relationships
        .iter()
        .filter(|relationship| relationship.evidence.is_empty())
        .count();
    let command_missing = evidence
        .commands
        .build
        .iter()
        .chain(evidence.commands.test.iter())
        .chain(evidence.commands.dev.iter())
        .filter(|command| command.source.as_os_str().is_empty())
        .count();
    fact_missing + repo_missing + relationship_missing + command_missing
}

fn collect_tier_counts(evidence: &WorkspaceDiscoveryEvidence) -> BTreeMap<NegentropyTier, usize> {
    let mut counts = BTreeMap::new();
    for fact in all_fact_refs(evidence) {
        *counts.entry(fact.tier).or_insert(0) += 1;
    }
    counts
}

fn tier_count(counts: &BTreeMap<NegentropyTier, usize>, tier: NegentropyTier) -> usize {
    counts.get(&tier).copied().unwrap_or(0)
}

fn all_fact_refs(evidence: &WorkspaceDiscoveryEvidence) -> Vec<&DiscoveryFact> {
    let mut facts = Vec::new();
    facts.extend(evidence.tech_stack.iter());
    facts.extend(evidence.api_contracts.iter());
    facts.extend(evidence.user_journeys.iter());
    facts.extend(evidence.e2e_test_cases.iter());
    facts.extend(evidence.auth.iter());
    facts.extend(evidence.coding_conventions.iter());
    facts.extend(evidence.project_intent.iter());
    facts.extend(evidence.environment_requirements.iter());
    facts
}

fn count_duplicate_display_items(evidence: &WorkspaceDiscoveryEvidence) -> usize {
    let fact_duplicate_count = [
        evidence.tech_stack.as_slice(),
        evidence.api_contracts.as_slice(),
        evidence.user_journeys.as_slice(),
        evidence.e2e_test_cases.as_slice(),
        evidence.auth.as_slice(),
        evidence.coding_conventions.as_slice(),
        evidence.project_intent.as_slice(),
        evidence.environment_requirements.as_slice(),
    ]
    .into_iter()
    .map(|facts| duplicate_count(facts.iter().map(|fact| fact.title.as_str())))
    .sum::<usize>();

    let command_duplicate_count = [
        evidence.commands.build.as_slice(),
        evidence.commands.test.as_slice(),
        evidence.commands.dev.as_slice(),
    ]
    .into_iter()
    .map(|commands| duplicate_count(commands.iter().map(|command| command.command.join(" "))))
    .sum::<usize>();

    fact_duplicate_count + command_duplicate_count
}

fn duplicate_count(items: impl IntoIterator<Item = impl Into<String>>) -> usize {
    let mut seen = BTreeSet::new();
    let mut duplicates = 0;
    for item in items {
        if !seen.insert(item.into()) {
            duplicates += 1;
        }
    }
    duplicates
}

fn count_confidence_ten_policy_violations(inference: &WorkspaceDiscoveryInference) -> usize {
    inference
        .inferences
        .iter()
        .filter(|entry| {
            entry.confidence == 10
                && !entry
                    .evidence_chains
                    .iter()
                    .any(|chain| chain.strength == DiscoveryEvidenceChainStrength::Strong)
        })
        .count()
}

fn count_heuristic_layer_rule_violations(
    evidence: Option<&WorkspaceDiscoveryEvidence>,
    inference: Option<&WorkspaceDiscoveryInference>,
    profile: Option<&WorkspaceProfile>,
) -> usize {
    let Some(evidence) = evidence else {
        return 0;
    };
    let supported_by_evidence = !evidence.layering.allowed_dependency_directions.is_empty();
    if supported_by_evidence {
        return 0;
    }

    let inferred_rule_count = inference.map_or(0, |item| {
        item.inferences
            .iter()
            .filter(|entry| entry.category == "layering_rule")
            .count()
    });
    let profile_rule_count =
        profile.map_or(0, |item| item.layering.allowed_dependency_directions.len());

    inferred_rule_count.max(profile_rule_count)
}

fn count_unsupported_profile_claims(
    evidence: Option<&WorkspaceDiscoveryEvidence>,
    inference: Option<&WorkspaceDiscoveryInference>,
    profile: Option<&WorkspaceProfile>,
) -> usize {
    let (Some(evidence), Some(profile)) = (evidence, profile) else {
        return 0;
    };

    let supported_rules = evidence
        .layering
        .allowed_dependency_directions
        .iter()
        .cloned()
        .chain(
            inference
                .into_iter()
                .flat_map(|item| item.inferences.iter())
                .filter(|entry| entry.category == "layering_rule")
                .map(|entry| entry.statement.clone()),
        )
        .collect::<BTreeSet<_>>();

    profile
        .layering
        .allowed_dependency_directions
        .iter()
        .filter(|rule| !supported_rules.contains(*rule))
        .count()
}

fn count_profile_faithfulness_mismatches(
    evidence: Option<&WorkspaceDiscoveryEvidence>,
    inference: Option<&WorkspaceDiscoveryInference>,
    profile: Option<&WorkspaceProfile>,
) -> usize {
    let (Some(evidence), Some(profile)) = (evidence, profile) else {
        return 0;
    };

    let expected = if let Some(inference) = inference {
        inference.assemble_profile(evidence)
    } else {
        WorkspaceDiscoveryRequest {
            scan: evidence.clone(),
            previous_profile: None,
            previous_inference: None,
        }
        .synthesize_profile()
    };

    let mut normalized_actual = profile.clone();
    let mut normalized_expected = expected;
    normalized_actual.generated_at = normalized_expected.generated_at;
    normalized_expected.generated_at = normalized_actual.generated_at;

    usize::from(normalized_actual != normalized_expected)
}

pub fn print_discovery_quality_report(
    report_path: &Path,
    report: &DiscoveryQualityReport,
) -> String {
    let mut lines = vec![
        format!("quality_report: {}", report_path.display()),
        format!("rubric_version: {}", report.rubric_version),
        format!("gate: {}", report.gate.as_str()),
        format!("overall_score: {}", report.overall_score),
        format!("workspace: {}", report.workspace_path.display()),
        format!("evaluated_at: {}", report.evaluated_at.to_rfc3339()),
        format!("summary: {}", report.summary),
    ];

    for dimension in &report.dimensions {
        lines.push(format!(
            "dimension: name={} score={} gate={} summary={}",
            dimension.name,
            dimension.score,
            dimension.gate.as_str(),
            dimension.summary
        ));
    }

    lines.push(format!(
        "metrics: stale_artifacts={} used_fallback_profile={} unsupported_profile_claim_count={} heuristic_layer_rule_count={} confidence_ten_policy_violations={}",
        report.metrics.stale_artifacts,
        report.metrics.used_fallback_profile,
        report.metrics.unsupported_profile_claim_count,
        report.metrics.heuristic_layer_rule_count,
        report.metrics.confidence_ten_policy_violations,
    ));

    for finding in &report.findings {
        lines.push(format!(
            "finding: severity={} code={} message={}",
            finding.severity.as_str(),
            finding.code,
            finding.message
        ));
    }

    for recommendation in &report.recommendations {
        lines.push(format!("recommendation: {recommendation}"));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{DiscoveryQualityGate, evaluate_discovery_artifacts};
    use crate::discovery::{
        ChangeBoundaryProfile, CommandCatalog, DiscoveryEvidenceChain,
        DiscoveryEvidenceChainStrength, DiscoveryInference, LayeringProfile,
        WorkspaceDiscoveryInference, WorkspaceDiscoveryPhase, WorkspaceDiscoveryStatus,
        WorkspaceProfile,
    };
    use chrono::Utc;
    use std::path::PathBuf;

    fn minimal_evidence() -> crate::discovery::WorkspaceDiscoveryEvidence {
        crate::discovery::WorkspaceDiscoveryEvidence {
            workspace_path: PathBuf::from("/tmp/workspace"),
            scanned_at: Utc::now(),
            workspace_fingerprint: "fp".to_string(),
            source_files: Vec::new(),
            tech_stack: Vec::new(),
            repositories: Vec::new(),
            dependency_relationships: Vec::new(),
            api_contracts: Vec::new(),
            layering: LayeringProfile {
                summary: "heuristic-only".to_string(),
                layers: Vec::new(),
                allowed_dependency_directions: Vec::new(),
                unresolved_ambiguities: vec!["heuristic only".to_string()],
            },
            user_journeys: Vec::new(),
            e2e_test_cases: Vec::new(),
            auth: Vec::new(),
            coding_conventions: Vec::new(),
            commands: CommandCatalog::default(),
            scan_notes: Vec::new(),
            project_intent: Vec::new(),
            environment_requirements: Vec::new(),
            change_boundaries: ChangeBoundaryProfile::default(),
        }
    }

    #[test]
    fn quality_report_flags_heuristic_layer_rule_promotion() {
        let evidence = minimal_evidence();
        let inference = WorkspaceDiscoveryInference {
            workspace_path: evidence.workspace_path.clone(),
            generated_at: Utc::now(),
            summary: "summary".to_string(),
            inferences: vec![DiscoveryInference {
                id: "inference.layering-rule.01".to_string(),
                category: "layering_rule".to_string(),
                statement: "Core must not depend on UI.".to_string(),
                confidence: 10,
                rationale: "bad".to_string(),
                evidence_chains: vec![DiscoveryEvidenceChain {
                    label: "guessed".to_string(),
                    strength: DiscoveryEvidenceChainStrength::Strong,
                    evidence_ids: vec!["ghost".to_string()],
                }],
                assumptions: Vec::new(),
                contradictions: Vec::new(),
            }],
            risks: Vec::new(),
        };
        let profile = WorkspaceProfile {
            workspace_path: evidence.workspace_path.clone(),
            generated_at: inference.generated_at,
            summary: "summary".to_string(),
            key_concepts: Vec::new(),
            tech_stack: Vec::new(),
            repositories: Vec::new(),
            dependency_relationships: Vec::new(),
            api_contracts: Vec::new(),
            layering: LayeringProfile {
                summary: "summary".to_string(),
                layers: Vec::new(),
                allowed_dependency_directions: vec!["Core must not depend on UI.".to_string()],
                unresolved_ambiguities: Vec::new(),
            },
            user_journeys: Vec::new(),
            e2e_test_cases: Vec::new(),
            auth: Vec::new(),
            coding_conventions: Vec::new(),
            commands: CommandCatalog::default(),
            risks: Vec::new(),
            project_intent: Vec::new(),
            environment_requirements: Vec::new(),
            change_boundaries: ChangeBoundaryProfile::default(),
        };
        let status = WorkspaceDiscoveryStatus {
            workspace_path: evidence.workspace_path.clone(),
            scan_path: PathBuf::from("scan.json"),
            evidence_path: PathBuf::from("evidence.json"),
            profile_path: PathBuf::from("profile.json"),
            inference_path: PathBuf::from("inference.json"),
            workspace_fingerprint: evidence.workspace_fingerprint.clone(),
            profile_fingerprint: Some("profile".to_string()),
            last_scanned_at: evidence.scanned_at,
            last_refreshed_at: Some(inference.generated_at),
            last_refresh_error: None,
            used_fallback_profile: false,
            current_phase: WorkspaceDiscoveryPhase::Ready,
            phase_heartbeat_at: Some(Utc::now()),
        };

        let report = evaluate_discovery_artifacts(
            Some(&status),
            Some(&evidence),
            Some(&inference),
            Some(&profile),
        )
        .expect("report");

        assert_eq!(report.gate, DiscoveryQualityGate::Fail);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == "heuristic_layer_rule_promotion")
        );
    }
}
