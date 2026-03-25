use crate::{
    config::EvaluatorConfig,
    domain::{
        BuilderHandoff, EvaluationRequest, FeatureContract, ScreenshotEvidence,
        VerificationEvidence,
    },
    runtime::RuntimePlan,
};

pub fn build_evaluation_request(
    contract: &FeatureContract,
    builder_handoff: &BuilderHandoff,
    runtime_plan: &RuntimePlan,
    config: &EvaluatorConfig,
    verification_evidence: VerificationEvidence,
    screenshot_evidence: Option<ScreenshotEvidence>,
) -> EvaluationRequest {
    EvaluationRequest {
        contract: contract.clone(),
        builder_handoff: builder_handoff.clone(),
        dimensions: config.dimensions.clone(),
        require_screenshots: config.require_screenshots,
        service_names: runtime_plan
            .services
            .iter()
            .map(|service| service.name.clone())
            .collect(),
        verification_commands: config.commands.clone(),
        verification_evidence,
        screenshot_evidence,
    }
}
