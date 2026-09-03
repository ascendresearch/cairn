use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

use cairn_agent::{
    AdapterVersion, AgentLoopStepLimit, EpisodeBudget, KnowledgeRegistry, ModelOutputTokenLimit,
    ModelSelection, ModelTemplate, ModelTemplateRegistry, RuntimeModelAlias, RuntimeModelCatalog,
    SkillRegistry,
};
use cairn_migration::{
    CandidateMechanismCatalogV1, CandidateSearchPolicyV1, OracleAdmissionPolicyV1,
    OracleAdversarialPolicyV1, OracleCoveragePolicyV1, OracleCoverageProfileV1,
    ReasoningDecompositionPolicyV1, SirTaskLimits, TaskIntentAuthoritySubject,
};
use cairn_migration_app::{
    CandidateBuildRunnerV1, CandidateBuildWorkerConfigV1, CudaMigrationApplication,
    CudaMigrationProductModuleV1, EvidenceExperimentWorkerConfigV1,
    MigrationAgentRuntimeExecutorV1, MigrationRoleAttemptLimitV1, MigrationRuntimeMaterialsV1,
    OracleControlRunnerV1, OracleControlWorkerConfigV1, migration_product_boundary,
    migration_tool_registry,
};
use cairn_server::{ApplicationName, load_server_config, run_with_application};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductConfigV1 {
    schema_version: u16,
    server_config: PathBuf,
    app_api_socket: PathBuf,
    authority_subject: TaskIntentAuthoritySubject,
    model_template: PathBuf,
    runtime_model_catalog: PathBuf,
    credential_base: PathBuf,
    runtime_model: RuntimeModelAlias,
    agent_event_database: PathBuf,
    agent_content_database: PathBuf,
    agent_content_directory: PathBuf,
    episode_budget: EpisodeBudget,
    model_output_tokens: ModelOutputTokenLimit,
    agent_loop_step_limit: AgentLoopStepLimit,
    migration_role_attempt_limit: MigrationRoleAttemptLimitV1,
    task_limits: SirTaskLimits,
    #[serde(default)]
    archive_limits: cairn_migration::TaskArchiveLimits,
    inbox_capacity: usize,
    oracle_coverage_profile: OracleCoverageProfileV1,
    oracle_adversarial_policy: OracleAdversarialPolicyV1,
    reasoning_decomposition: ReasoningDecompositionPolicyV1,
    evidence_experiment_worker: Option<EvidenceExperimentWorkerConfigV1>,
    oracle_control_worker: OracleControlWorkerConfigV1,
    candidate_build_worker: Option<CandidateBuildWorkerConfigV1>,
    candidate_mechanisms: Option<CandidateMechanismCatalogV1>,
    #[serde(default)]
    candidate_search: CandidateSearchPolicyV1,
}

#[tokio::main]
async fn main() -> ExitCode {
    if cairn_observability::init("cairn-cuda-migration-server").is_err() {
        eprintln!("CUDA migration server logging initialization failed");
        return ExitCode::FAILURE;
    }
    match Box::pin(run()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_error) => {
            tracing::error!(
                target: "cairn.migration.process",
                event = "cuda_migration_process_failed",
                error_class = "startup-or-runtime",
                "CUDA migration product process terminated"
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: cairn-cuda-migration-server PRODUCT.json")?;
    if env::args_os().nth(2).is_some() {
        return Err("usage: cairn-cuda-migration-server PRODUCT.json".into());
    }
    let base = config_path.parent().unwrap_or_else(|| Path::new("."));
    let mut config: ProductConfigV1 = serde_json::from_slice(&std::fs::read(&config_path)?)?;
    if config.schema_version != 1 || config.inbox_capacity == 0 {
        return Err("invalid current-V1 CUDA migration product configuration".into());
    }
    if config.reasoning_decomposition.permits_worker_experiments()
        && config.evidence_experiment_worker.is_none()
    {
        return Err(
            "evidence-augmented reasoning requires an ordinary Worker configuration".into(),
        );
    }
    resolve_product_paths(&mut config, base);
    let server = load_server_config(&config.server_config)?;
    let template: ModelTemplate = serde_json::from_slice(&std::fs::read(&config.model_template)?)?;
    let templates = ModelTemplateRegistry::from_templates([template])?;
    let catalog: RuntimeModelCatalog =
        serde_json::from_slice(&std::fs::read(&config.runtime_model_catalog)?)?;
    let model = catalog.resolve(&templates, Some(&config.runtime_model))?;
    let selection = ModelSelection {
        provider: model.provider().clone(),
        model: model.wire_model().clone(),
        deployment: model.deployment().clone(),
        adapter_version: AdapterVersion::new("native-protocol-v1")?,
    };
    let materials = MigrationRuntimeMaterialsV1::default();
    let executor = MigrationAgentRuntimeExecutorV1::open(
        model,
        selection,
        config.episode_budget,
        config.model_output_tokens,
        config.credential_base,
        &config.agent_event_database,
        &config.agent_content_database,
        &config.agent_content_directory,
        &server.content_database(),
        &server.content_directory(),
        materials.clone(),
        config
            .evidence_experiment_worker
            .map(|worker| (server.clone(), worker)),
    )?;
    let oracle_policy = OracleCoveragePolicyV1::new(
        config.oracle_coverage_profile,
        config.oracle_adversarial_policy,
    );
    let oracle_catalog = executor.oracle_strategy_catalog(&oracle_policy)?;
    let oracle_controls = OracleControlRunnerV1::new(server.clone(), config.oracle_control_worker)?;
    let candidate_build = config
        .candidate_build_worker
        .map(|worker| CandidateBuildRunnerV1::new(server.clone(), worker))
        .transpose()?;
    let (api, services, inbox) = migration_product_boundary(
        config.app_api_socket,
        &config.authority_subject,
        config.task_limits,
        config.archive_limits,
        materials,
        oracle_policy,
        oracle_catalog,
        oracle_controls,
        candidate_build,
        config.reasoning_decomposition,
        server.clone(),
        config.candidate_search,
        config.inbox_capacity,
    )?;
    let name = ApplicationName::new("cuda-migration")?;
    let workflow = CudaMigrationApplication::new(
        name.clone(),
        services,
        executor,
        inbox,
        migration_tool_registry()?,
        SkillRegistry::default(),
        KnowledgeRegistry::default(),
        config.agent_loop_step_limit,
        config.migration_role_attempt_limit,
        OracleAdmissionPolicyV1::strict(),
        config.candidate_mechanisms,
    );
    let product = CudaMigrationProductModuleV1::new(name, api, workflow);
    run_with_application(server, product).await?;
    Ok(())
}

fn resolve_product_paths(config: &mut ProductConfigV1, base: &Path) {
    for path in [
        &mut config.server_config,
        &mut config.app_api_socket,
        &mut config.model_template,
        &mut config.runtime_model_catalog,
        &mut config.credential_base,
        &mut config.agent_event_database,
        &mut config.agent_content_database,
        &mut config.agent_content_directory,
    ] {
        if path.is_relative() {
            *path = base.join(&*path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProductConfigV1;

    /// The shipped example has to be readable by the type that will read it.
    ///
    /// An example that only looks like the configuration is worse than none: it is copied, edited,
    /// deployed, and then fails at startup on a live host, which is where this project has already
    /// lost a system once.
    #[test]
    fn the_shipped_product_example_parses_as_the_configuration_it_documents() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/migration-product.example.json"
        ))
        .expect("example configuration");

        let config: ProductConfigV1 =
            serde_json::from_slice(&bytes).expect("the example is the current configuration");

        assert_eq!(config.schema_version, 1);
        assert!(config.inbox_capacity > 0);
        // The build recipe is deployment material the Controller reads at startup, so an example
        // naming a path that does not exist would fail only once a candidate was already waiting.
        let runner = config
            .candidate_build_worker
            .as_ref()
            .expect("the example configures a candidate build worker")
            .runner_path();
        assert!(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../"))
                .join(runner)
                .is_file(),
            "the example names a build recipe that is not in the tree: {}",
            runner.display()
        );
    }
}
