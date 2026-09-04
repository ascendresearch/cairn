use std::{
    collections::BTreeMap,
    future::{Future, ready},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use cairn_execution::{ExecutionStderrArtifact, ExecutionStdoutArtifact};

use cairn_agent::{
    AgentEpisodeDriverError, AgentLoopCheckpointV1, AgentLoopExhaustionReasonV1,
    AgentLoopStepExecutionV1, AgentLoopStepExecutor, AgentStepAccessV1, CanonicalToolResult,
    ContextBlock, EpisodeBudget, FrozenAgentEpisodeDriverV1, HistoryItem, HttpModelTransport,
    InstructionBlock, ModelOutputTokenLimit, ModelSelection, NativeProtocolCodec,
    NativeRequestSpec, NativeToolDefinition, PolicyDocument, PreparedToolOperation,
    ResolvedRuntimeModel, ToolCatalog, ToolDescriptorArtifact, ToolEffectClass, ToolGateway,
    ToolGatewayError, ToolImplementationVersion, ToolRegistration, ToolRegistry,
    TransportFailureClass, drive_agent_episode_step,
};
use cairn_migration::{
    AgentLoopRuntimeBindingArtifact, AgentResolvedRuntimeModelArtifact, AuthoritativeIntentClaimV1,
    CandidateExplorationAgentContextV1, CandidateOracleContractV1, CandidateProposalSubmissionV1,
    CandidateProposalV1, CandidateRevisionAgentContextV1, CandidateWorkspaceV1,
    IntentHypothesisSetProposalV1, IntentRecoveryInputV1, MigrationAgentToolV1,
    MigrationRoleStepObservationV1, OracleBuildTestSnapshotArtifact, OracleCheckAssertionV1,
    OracleCheckEvidenceV1, OracleCheckMethodV1, OracleCheckObjective, OracleCheckObservation,
    OracleCheckPassCondition, OracleCheckPlanV1, OracleCheckSetup, OracleClaimV1,
    OracleControlFailureClassV1, OracleControlResultV1, OracleCoveragePolicyV1,
    OracleDimensionItemDiscoveryAgentContextV1, OracleDimensionItemSetProposalV1,
    OracleDimensionItemSetReviewDecisionV1, OracleDimensionItemSetReviewV1,
    OracleDimensionItemSetReviewerAgentContextV1, OracleDimensionV1,
    OracleDocumentationSnapshotArtifact, OracleExperimentLimit,
    OracleExperimentToolCatalogArtifact, OracleExplorationBudgetV1,
    OracleExplorationCapabilityGrantArtifact, OracleItemDeveloperAgentContextV1,
    OracleItemDiscoveryRevisionLimit, OracleItemDraftV1, OracleItemReviewDecisionV1,
    OracleItemReviewV1, OracleItemReviewerAgentContextV1, OracleItemRevisionLimit,
    OracleItemStatement, OracleItemV1, OracleKnowledgeSnapshotArtifact,
    OraclePortfolioCoherenceDecisionV1, OraclePortfolioCoherenceReviewV1,
    OraclePortfolioCoherenceReviewerAgentContextV1, OraclePortfolioProposalV1,
    OracleResearchToolCatalogArtifact, OracleRevisionRequestV1, OracleSourceSnapshotArtifact,
    OracleStrategyCatalogV1, OracleStrategyExecutorV1, OracleStrategyKindV1, OracleStrategyName,
    OracleStrategyRegistrationV1, OracleStrategyRoleV1, OracleStrategyRunLimit,
    OracleStrategyRunV1, OracleStrategyToolCatalogV1, OracleWholePortfolioAgentContextV1,
    OracleWholePortfolioProposalAuthorityV1, OracleWorkspaceInput, OracleWorkspaceV1,
    SirAgentContextV1, SirProposalSubmissionV1, SirReadLineLimit, SirSourceLineNumber,
    SirTaskArtifactPath, SirTaskLimits, SirTaskWorkspace, TrustedOracleControlReceiptArtifact,
    derive_oracle_claims, derive_oracle_dimensions,
};
use cairn_protocol::{AgentLoopId, ContentId, ContentType, EpisodeId, TaskId};
use cairn_record::ContentStore;
use cairn_server::ServerConfig;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_verification::ModelConfigurationArtifact;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;

use crate::evidence_experiment_runner::{
    EvidenceExperimentRunnerV1, EvidenceExperimentWorkerConfigV1,
};

const SCHEMA_V1: u16 = 1;
const TOOL_VERSION: &str = "migration-role-tools-v1";
const MODEL_BACKED_SYNTHESIS_STRATEGY: &str = "model-backed-synthesis";

/// The strategy roles this product actually registers.
///
/// Named once because two sides read it: the catalog that registers the strategies, and the check
/// that a deployment's coverage policy does not ask for a role nothing implements. A shipped
/// example demanded the adversarial role against this list and every proposal was refused after a
/// model call had been paid for. Adding an adversarial strategy means extending this and the
/// registration together.
pub(crate) const REGISTERED_STRATEGY_ROLES: &[OracleStrategyRoleV1] =
    &[OracleStrategyRoleV1::Synthesis];
const SIR_INSTRUCTION: &str = r"You are the semantic-intent-recovery analyst for one CUDA-to-Ascend-C migration task.

Inspect only the offered task artifacts. First use migration-read-task-artifact to read the source, host launch, ABI, tests, or build files needed for your analysis. Treat observable source facts separately from intent inferences. Cite exact task-local paths and inclusive line ranges.

The caller declaration is an attributed authority source, not a fact to overwrite. Keep caller claims separate from source observations and from your hypotheses. Submit exactly one complete proposal through migration-submit-sir. It must contain source-observed facts with citations, at least two genuinely competing hypotheses, an explicit conflict, at least one unknown, and at least one evidence-backed invariant. When a conflict cannot be resolved by offered evidence and requires the task authority, include an unknown for that exact decision and a disambiguation experiment that jointly targets the unknown plus either the conflict itself or at least two hypotheses named by that conflict. Classify the unknown by its real subject. Also report applicable optimization freedoms, source-behavior dispositions, and disambiguation experiments; use empty arrays when none are justified. Every reference must point to an ID declared in this proposal or the frozen caller declaration. Use lowercase kebab-case local IDs and sort every top-level collection lexicographically by its id.

The proposal is non-authoritative. Do not claim admission, correctness, a confidence score, or a migration verdict. Do not invent content identities or use paths outside the offered task bundle.";

const ORACLE_ITEM_DISCOVERY_INSTRUCTION: &str = r"You discover independently reviewable Oracle items for one exact Controller-derived dimension.

Read the offered dimension and its exact admitted-intent claim through migration-read-oracle-dimension. The frozen context lists every valid task-artifact path; use migration-read-task-artifact only for those paths and never guess a path for a Controller object. Decompose the dimension into one or more distinct, concrete obligations that together express what this dimension needs checked. Every item must be candidate-facing: it must be capable of producing an expected value, property, comparator, candidate execution obligation, safety obligation, or performance requirement that can judge a future Ascend-C candidate or its target execution receipt. A statement that only restates the admitted intent, characterizes the CUDA source, or says that implementation details are free is not an Oracle item. CUDA facts may support a candidate-facing item but cannot be the only subject being checked. On a revision, read and address every exact item-set Review finding without changing the dimension. Submit only item statements through migration-submit-oracle-dimension-items. Do not design check plans yet, merge dimensions, invent identities, claim review or admission, or use unavailable knowledge.";

const ORACLE_WHOLE_PORTFOLIO_INSTRUCTION: &str = r"You propose one complete Oracle portfolio for a CUDA-to-Ascend-C migration in a single Agent Loop.

Read migration-read-oracle-whole-portfolio-scope before submitting. It provides every exact Controller-derived dimension, admitted-intent claim, target context, and exact prior Admission feedback when this is a revision. Read offered task files through migration-read-task-artifact as needed. Submit every offered dimension exactly once through migration-submit-oracle-whole-portfolio. For each dimension provide one or more distinct candidate-facing items, and for every item provide one or more executable plans with an obtainable future Ascend-C candidate artifact or target observation, an unambiguous pass condition, and exact source-citation or admitted-intent evidence. CUDA source characterization may support a plan but cannot be its only candidate observation. Resolve consistency, overlap, domain, numerical, integration, safety, and performance interactions yourself within this one episode; no model Reviewer will repair the result. Preserve unknowns rather than inventing evidence. Do not invent identities, request unavailable Worker experiments, claim execution, qualification, Review, or Admission.";

const ORACLE_ITEM_SET_REVIEW_INSTRUCTION: &str = r"You independently review one exact proposed item decomposition for one Controller-derived Oracle dimension.

Read migration-read-oracle-dimension-items, including the full Controller-derived dimension and its exact admitted-intent claim. The frozen context lists every valid task-artifact path; use migration-read-task-artifact only for those paths and never guess a path for a Controller object. Approve only if the items are concrete, non-overlapping, remain inside the exact dimension, jointly cover that dimension, and can judge a future Ascend-C candidate or its target execution receipt. Reject an item as not-candidate-facing when it only restates intent, characterizes CUDA, proves that a CUDA mechanism has some property, or lists implementation freedoms without defining a candidate-facing expected value, property, comparator, execution obligation, safety obligation, or performance requirement. Reject unsupported or unreadable evidence with actionable findings through migration-submit-oracle-dimension-items-review. Do not design check plans, rewrite the item set yourself, or claim control or Admission authority.";

const ORACLE_ITEM_DEVELOPMENT_INSTRUCTION: &str = r"You develop one exact Oracle item for a CUDA-to-Ascend-C migration task.

Read migration-read-oracle-item-conversation, including the item's exact dimension and admitted-intent claim, before submitting. The frozen context lists every valid task-artifact path; use migration-read-task-artifact only for those paths and never guess a path for a Controller object. On the initial revision, create one or more complementary, executable check plans for only the offered item. Every plan must say what future Ascend-C candidate artifact or target execution observation it consumes and how it can accept or reject that candidate. Static analysis of the CUDA source or restatement of admitted intent can support a plan but cannot itself be the candidate observation. On a later revision, preserve the same item and address every finding from the exact prior draft review and every exact artifact-owned failed control supplied by Admission. For each failed receipt, call migration-read-oracle-control-diagnostic and use its bounded exact stdout/stderr to determine the required correction; do not guess from an exit code or artifact identity. Treat prior receipts only as feedback, never as passing authority. Negative-challenge, mechanism, infrastructure-unavailable, or missing observations are reconciled by the Controller and are never a reason to rewrite an item. Each plan must state an objective, setup, obtainable candidate observation, unambiguous pass condition, and exact source citation or admitted-intent evidence. Each plan must also carry an assertion, which is the machine-evaluable half of its pass condition: the comparator a runner would compute, and where its tolerance came from. Use exact-bytes only when the observation really is bit-reproducible. Pair any tolerance with the origin that justifies it rather than a number you picked, because a tolerance nobody can account for cannot say how wrong a candidate would have to be before the check complains. Submit only through migration-submit-oracle-item-draft. Do not change the item, omit feedback, claim execution, review, qualification, or admission.";

const CANDIDATE_REVISION_INSTRUCTION: &str = r"You revise one Ascend C implementation that a build has just refused.

Read migration-read-candidate-observation first. It carries the exact previous proposal and the compiler's own stdout and stderr for the build that refused it. Determine the required correction from that text; an exit code, a receipt identity, or the shape of the code alone does not tell you what the toolchain objected to, and an API invented to satisfy a guess will fail the next build the same way. If the diagnostic does not identify the cause, say so in the explanation rather than changing something at random.

Read migration-read-admitted-oracle to keep the admitted intent claims in view: a revision that compiles by dropping required behaviour has not made progress. Submit the complete revised source through migration-submit-candidate-revision, including every file the build needs and not only the ones you changed. State in the explanation what the diagnostic said, what you changed because of it, and any assumption you still could not verify.

The observation also reports how many build attempts remain and, when the Controller has something to tell you about the search itself, a notice. A notice that your previous submission repeated one already built means that exact source has already been refused: submitting it again spends nothing and changes nothing. You have no build, execution, review, qualification, or admission authority.";

const CANDIDATE_EXPLORATION_INSTRUCTION: &str = r"You write the first Ascend C implementation of one CUDA operator for a migration task.

Read migration-read-admitted-oracle before anything else. It carries the admitted intent claims this implementation has to satisfy and is the only statement of required behaviour you are authorized to rely on. The frozen context lists every valid task-artifact path; use migration-read-task-artifact only for those paths and never guess a path for a Controller object. The CUDA source tells you what the original program did, which is evidence about intent and not a specification: do not reproduce a source defect, an accidental launch behaviour, or an unnecessary numerical error merely because you observed it there.

Aim first at the simplest implementation that is correct and can be checked, not at the fastest one. Write complete source that a build can compile without further editing: no placeholder bodies, no omitted declarations, no pseudocode, and no file the build would need but you did not submit. State in the explanation every assumption you could not verify from the task artifacts or the admitted claims, and say what observation would settle it. An assumption you state is a known unknown; an assumption you leave silent becomes a defect someone else has to find.

Submit exactly one proposal through migration-submit-candidate, listing every file the build needs and naming the one that holds the kernel entry point. You have no build, execution, review, qualification, or admission authority, and submitting a proposal is never evidence that it compiles or runs.";

const ORACLE_ITEM_REVIEW_INSTRUCTION: &str = r"You independently review one exact Oracle item draft revision.

Read migration-read-oracle-item-draft, including the item's exact dimension and admitted-intent claim. The frozen context lists every valid task-artifact path; use migration-read-task-artifact only for those paths and never guess a path for a Controller object. Inspect every exact source range needed to verify the draft's source citations; never treat an unread citation as support. Approve only if every proposed plan addresses the exact item, is supported by readable cited evidence, has complete setup, consumes an obtainable future Ascend-C candidate artifact or target execution observation, and has an unambiguous candidate acceptance condition. A plan that only inspects CUDA, restates intent, or describes implementation freedom has no candidate observation and must be rejected as observation-unexecutable. Otherwise submit one or more actionable findings bound to this exact item and draft through migration-submit-oracle-item-review. Multiple distinct findings may use the same issue class. Do not redesign the plan yourself or claim qualification or admission.";

const ORACLE_PORTFOLIO_COHERENCE_REVIEW_INSTRUCTION: &str = r"You independently review only the relationships among already item-reviewed Oracle drafts in one exact portfolio.

Read migration-read-oracle-portfolio, including the complete exact admitted-claim and Controller-dimension inventory. Check for contradictory items, duplicate coverage, conflicting pass conditions, cross-plane gaps, and failures of the items to provide coherent joint coverage. Do not redo each item's detailed plan review and do not generate plans. Approve only when the exact assembled portfolio is coherent. Otherwise submit actionable findings through migration-submit-oracle-portfolio-coherence-review; every finding must name a non-empty exact affected item set, an issue class, an explanation, and a required change. You have no control, receipt, or Admission authority.";

fn role_instruction(base: &str, policy: cairn_migration::ReasoningDecompositionPolicyV1) -> String {
    if policy.permits_worker_experiments() {
        format!(
            "{base}\n\nThe current task grants migration-run-evidence-experiment. Use it only when a bounded executable Worker observation can discriminate an explicit uncertainty or verify a material Review concern. The required language is posix-shell: program must be POSIX /bin/sh source, not Python or another interpreter's source. State the purpose in the request, inspect the returned exact receipt, and cite the observation in your reasoning before submitting. Do not request tautological source-printing, claim unavailable CUDA/GPU capabilities, or treat a proposal experiment as Oracle Admission authority."
        )
    } else {
        base.to_owned()
    }
}

enum RuntimeRoleSubmissionV1<T> {
    Submitted(T),
    Exhausted(AgentLoopExhaustionReasonV1),
}

fn resolve_role_submission<T>(
    submission: Option<T>,
    reason: cairn_agent::EpisodeCompletionReason,
    label: &'static str,
) -> Result<RuntimeRoleSubmissionV1<T>, MigrationAgentRuntimeError> {
    if let Some(submission) = submission {
        return Ok(RuntimeRoleSubmissionV1::Submitted(submission));
    }
    let reason = match reason {
        cairn_agent::EpisodeCompletionReason::Yielded => {
            return Err(MigrationAgentRuntimeError::MissingSubmission(label));
        }
        cairn_agent::EpisodeCompletionReason::StepLimitReached => {
            AgentLoopExhaustionReasonV1::EpisodeStepLimit
        }
        cairn_agent::EpisodeCompletionReason::DeadlineReached => {
            AgentLoopExhaustionReasonV1::EpisodeDeadline
        }
        cairn_agent::EpisodeCompletionReason::ToolOperationLimitReached => {
            AgentLoopExhaustionReasonV1::EpisodeToolOperationLimit
        }
        cairn_agent::EpisodeCompletionReason::ProviderTokenLimitReached => {
            AgentLoopExhaustionReasonV1::EpisodeProviderTokenLimit
        }
        cairn_agent::EpisodeCompletionReason::ProviderUsageUnavailable => {
            AgentLoopExhaustionReasonV1::EpisodeProviderUsageUnavailable
        }
    };
    Ok(RuntimeRoleSubmissionV1::Exhausted(reason))
}

#[derive(Clone)]
struct RuntimeTaskV1 {
    reasoning_decomposition: cairn_migration::ReasoningDecompositionPolicyV1,
    workspace: SirTaskWorkspace,
    recovery_input: IntentRecoveryInputV1,
    limits: SirTaskLimits,
    oracle: Option<RuntimeOracleTaskV1>,
    candidate: Option<RuntimeCandidateTaskV1>,
}

/// Frozen Candidate authority a proposal episode may read.
///
/// The contract is the admitted-Oracle projection the candidate is allowed to see, and the
/// workspace is the frozen material authority it may read from. Both are minted by the Controller
/// before the first episode, so an episode cannot widen what it is allowed to consult.
#[derive(Clone)]
struct RuntimeCandidateTaskV1 {
    workspace: CandidateWorkspaceV1,
    contract: CandidateOracleContractV1,
}

#[derive(Clone)]
struct RuntimeOracleTaskV1 {
    workspace: OracleWorkspaceV1,
    policy: OracleCoveragePolicyV1,
    catalog: OracleStrategyCatalogV1,
    admitted_claims: Vec<AuthoritativeIntentClaimV1>,
    revision_requests: Vec<OracleRevisionRequestV1>,
    item_sets: Vec<OracleDimensionItemSetProposalV1>,
    item_set_reviews: Vec<OracleDimensionItemSetReviewV1>,
    item_drafts: Vec<OracleItemDraftV1>,
    item_reviews: Vec<OracleItemReviewV1>,
    portfolios: Vec<OraclePortfolioProposalV1>,
    coherence_reviews: Vec<OraclePortfolioCoherenceReviewV1>,
}

/// Exact product material made available to role executors by task identity.
#[derive(Clone, Default)]
pub struct MigrationRuntimeMaterialsV1(Arc<RwLock<BTreeMap<TaskId, RuntimeTaskV1>>>);

impl MigrationRuntimeMaterialsV1 {
    /// Registers the frozen task snapshot before its SIR Agent Loop starts.
    ///
    /// # Errors
    ///
    /// Rejects identity drift or a second, different snapshot for the same task.
    pub fn register_task(
        &self,
        task_id: TaskId,
        workspace: SirTaskWorkspace,
        recovery_input: IntentRecoveryInputV1,
        limits: SirTaskLimits,
        reasoning_decomposition: cairn_migration::ReasoningDecompositionPolicyV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        if recovery_input.task_id() != task_id
            || recovery_input.task_bundle()
                != workspace
                    .bundle()
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        if let Some(existing) = tasks.get(&task_id) {
            if existing.recovery_input != recovery_input
                || existing.workspace.bundle() != workspace.bundle()
                || existing.reasoning_decomposition != reasoning_decomposition
            {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
            return Ok(());
        }
        tasks.insert(
            task_id,
            RuntimeTaskV1 {
                reasoning_decomposition,
                workspace,
                recovery_input,
                limits,
                oracle: None,
                candidate: None,
            },
        );
        Ok(())
    }

    /// Freezes the Candidate authority before the first Candidate proposal episode.
    ///
    /// # Errors
    ///
    /// Rejects an unknown task, a poisoned lock, or a second, different Candidate authority.
    pub fn register_candidate(
        &self,
        task_id: TaskId,
        workspace: CandidateWorkspaceV1,
        contract: CandidateOracleContractV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let task = tasks
            .get_mut(&task_id)
            .ok_or(MigrationAgentRuntimeError::UnknownTask(task_id))?;
        if let Some(existing) = &task.candidate {
            if existing.workspace != workspace || existing.contract != contract {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
            return Ok(());
        }
        task.candidate = Some(RuntimeCandidateTaskV1 {
            workspace,
            contract,
        });
        Ok(())
    }

    /// Freezes the product-selected Oracle workspace and the admitted claim it may inspect.
    ///
    /// # Errors
    ///
    /// Rejects unknown tasks, identity drift, invalid policy/catalog data, or poisoned state.
    #[allow(
        clippy::too_many_lines,
        reason = "workspace registration keeps every exact unavailable material, capability, policy, catalog, and budget edge visible"
    )]
    pub fn register_oracle(
        &self,
        task_id: TaskId,
        admitted_intent: ContentId<cairn_migration::MigrationIntentContractArtifact>,
        admitted_claims: Vec<AuthoritativeIntentClaimV1>,
        policy: OracleCoveragePolicyV1,
        catalog: OracleStrategyCatalogV1,
    ) -> Result<OracleWorkspaceV1, MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let task = tasks
            .get_mut(&task_id)
            .ok_or(MigrationAgentRuntimeError::UnknownTask(task_id))?;
        let policy_id = policy
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let catalog_id = catalog
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let bundle_bytes = cairn_codec::to_vec(task.workspace.bundle())
            .map_err(MigrationAgentRuntimeError::domain)?;
        let unavailable = |subject: &'static str| {
            cairn_codec::to_vec(&json!({
                "schema_version": SCHEMA_V1,
                "availability": "not-provided",
                "subject": subject,
            }))
            .map_err(MigrationAgentRuntimeError::domain)
        };
        let budget = OracleExplorationBudgetV1 {
            strategy_runs: OracleStrategyRunLimit::new(
                u32::try_from(policy.concerns().len())
                    .map_err(MigrationAgentRuntimeError::domain)?,
            )
            .map_err(MigrationAgentRuntimeError::domain)?,
            experiments: OracleExperimentLimit::new(1)
                .map_err(MigrationAgentRuntimeError::domain)?,
            item_discovery_revisions: cairn_migration::OracleItemDiscoveryRevisionLimit::new(4)
                .map_err(MigrationAgentRuntimeError::domain)?,
            item_revisions: OracleItemRevisionLimit::new(4)
                .map_err(MigrationAgentRuntimeError::domain)?,
        };
        let workspace = OracleWorkspaceV1::new(&OracleWorkspaceInput {
            task_id,
            admitted_intent,
            sir_input: task
                .recovery_input
                .identity()
                .map_err(MigrationAgentRuntimeError::domain)?,
            sir_task_bundle: task
                .workspace
                .bundle()
                .identity()
                .map_err(MigrationAgentRuntimeError::domain)?,
            source: ContentId::<OracleSourceSnapshotArtifact>::derive(&bundle_bytes)
                .map_err(MigrationAgentRuntimeError::domain)?,
            documentation: ContentId::<OracleDocumentationSnapshotArtifact>::derive(&unavailable(
                "documentation",
            )?)
            .map_err(MigrationAgentRuntimeError::domain)?,
            build_and_tests: ContentId::<OracleBuildTestSnapshotArtifact>::derive(&bundle_bytes)
                .map_err(MigrationAgentRuntimeError::domain)?,
            knowledge: ContentId::<OracleKnowledgeSnapshotArtifact>::derive(&unavailable(
                "knowledge",
            )?)
            .map_err(MigrationAgentRuntimeError::domain)?,
            research_tools: ContentId::<OracleResearchToolCatalogArtifact>::derive(&unavailable(
                "research-tools",
            )?)
            .map_err(MigrationAgentRuntimeError::domain)?,
            experiment_tools: ContentId::<OracleExperimentToolCatalogArtifact>::derive(
                &unavailable("experiment-tools")?,
            )
            .map_err(MigrationAgentRuntimeError::domain)?,
            capability_grant: ContentId::<OracleExplorationCapabilityGrantArtifact>::derive(
                &cairn_codec::to_vec(&json!({
                    "schema_version": SCHEMA_V1,
                    "tools": [
                        "migration-read-task-artifact",
                        "migration-read-oracle-dimension",
                        "migration-submit-oracle-dimension-items",
                        "migration-read-oracle-dimension-items",
                        "migration-submit-oracle-dimension-items-review",
                        "migration-read-oracle-item-conversation",
                        "migration-submit-oracle-item-draft",
                        "migration-read-oracle-item-draft",
                        "migration-submit-oracle-item-review",
                        "migration-read-oracle-portfolio",
                        "migration-submit-oracle-portfolio-coherence-review"
                    ],
                }))
                .map_err(MigrationAgentRuntimeError::domain)?,
            )
            .map_err(MigrationAgentRuntimeError::domain)?,
            coverage_policy: policy_id,
            strategy_catalog: catalog_id,
            budget,
        });
        if let Some(existing) = &task.oracle {
            if existing.workspace != workspace
                || existing.policy != policy
                || existing.catalog != catalog
                || existing.admitted_claims != admitted_claims
            {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
            return Ok(existing.workspace.clone());
        }
        task.oracle = Some(RuntimeOracleTaskV1 {
            workspace: workspace.clone(),
            policy,
            catalog,
            admitted_claims,
            revision_requests: Vec::new(),
            item_sets: Vec::new(),
            item_set_reviews: Vec::new(),
            item_drafts: Vec::new(),
            item_reviews: Vec::new(),
            portfolios: Vec::new(),
            coherence_reviews: Vec::new(),
        });
        Ok(workspace)
    }

    fn record_oracle_item_set(
        &self,
        task_id: TaskId,
        proposal: &OracleDimensionItemSetProposalV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get_mut(&task_id)
            .and_then(|task| task.oracle.as_mut())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        let identity = proposal
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if let Some(existing) = oracle
            .item_sets
            .iter()
            .find(|existing| existing.identity().is_ok_and(|value| value == identity))
        {
            if existing != proposal {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
        } else {
            if let Some(parent_id) = proposal.parent() {
                let parent = oracle
                    .item_sets
                    .iter()
                    .find(|existing| {
                        existing
                            .identity()
                            .is_ok_and(|identity| identity == parent_id)
                    })
                    .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
                if parent.dimension() != proposal.dimension()
                    || proposal.revision().get() != parent.revision().get() + 1
                {
                    return Err(MigrationAgentRuntimeError::TaskBinding);
                }
            }
            oracle.item_sets.push(proposal.clone());
        }
        Ok(())
    }

    fn record_oracle_item_set_review(
        &self,
        task_id: TaskId,
        review: &OracleDimensionItemSetReviewV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get_mut(&task_id)
            .and_then(|task| task.oracle.as_mut())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        let proposal = oracle
            .item_sets
            .iter()
            .find(|proposal| {
                proposal
                    .identity()
                    .is_ok_and(|identity| identity == review.proposal())
            })
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        review
            .validate_against(proposal)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let identity = review
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if let Some(existing) = oracle
            .item_set_reviews
            .iter()
            .find(|existing| existing.identity().is_ok_and(|value| value == identity))
        {
            if existing != review {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
        } else {
            oracle.item_set_reviews.push(review.clone());
        }
        Ok(())
    }

    fn record_oracle_item_draft(
        &self,
        task_id: TaskId,
        draft: &OracleItemDraftV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get_mut(&task_id)
            .and_then(|task| task.oracle.as_mut())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        let identity = draft
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if let Some(existing) = oracle
            .item_drafts
            .iter()
            .find(|existing| existing.identity().is_ok_and(|value| value == identity))
        {
            if existing != draft {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
        } else {
            oracle.item_drafts.push(draft.clone());
        }
        Ok(())
    }

    fn record_oracle_item_review(
        &self,
        task_id: TaskId,
        review: &OracleItemReviewV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get_mut(&task_id)
            .and_then(|task| task.oracle.as_mut())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        let identity = review
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if let Some(existing) = oracle
            .item_reviews
            .iter()
            .find(|existing| existing.identity().is_ok_and(|value| value == identity))
        {
            if existing != review {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
        } else {
            oracle.item_reviews.push(review.clone());
        }
        Ok(())
    }

    pub(crate) fn record_oracle_portfolio(
        &self,
        task_id: TaskId,
        portfolio: &OraclePortfolioProposalV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get_mut(&task_id)
            .and_then(|task| task.oracle.as_mut())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if portfolio.workspace()
            != oracle
                .workspace
                .identity()
                .map_err(MigrationAgentRuntimeError::domain)?
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let identity = portfolio
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if let Some(existing) = oracle
            .portfolios
            .iter()
            .find(|existing| existing.identity().is_ok_and(|value| value == identity))
        {
            if existing != portfolio {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
        } else {
            oracle.portfolios.push(portfolio.clone());
        }
        Ok(())
    }

    fn record_oracle_coherence_review(
        &self,
        task_id: TaskId,
        review: &OraclePortfolioCoherenceReviewV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get_mut(&task_id)
            .and_then(|task| task.oracle.as_mut())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        let portfolio = oracle
            .portfolios
            .iter()
            .find(|portfolio| {
                portfolio
                    .identity()
                    .is_ok_and(|identity| identity == review.portfolio())
            })
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        review
            .validate_against(portfolio)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let identity = review
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if let Some(existing) = oracle
            .coherence_reviews
            .iter()
            .find(|existing| existing.identity().is_ok_and(|value| value == identity))
        {
            if existing != review {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
        } else {
            oracle.coherence_reviews.push(review.clone());
        }
        Ok(())
    }

    pub(crate) fn record_oracle_revision_request(
        &self,
        task_id: TaskId,
        request: &OracleRevisionRequestV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let mut tasks = self
            .0
            .write()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get_mut(&task_id)
            .and_then(|task| task.oracle.as_mut())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        let request_id = request
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if let Some(existing) = oracle.revision_requests.iter().find(|existing| {
            existing
                .identity()
                .is_ok_and(|identity| identity == request_id)
        }) {
            if existing != request {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
        } else {
            oracle.revision_requests.push(request.clone());
        }
        Ok(())
    }

    pub(crate) fn oracle_check_plans(
        &self,
        task_id: TaskId,
        proposal: &OraclePortfolioProposalV1,
    ) -> Result<Vec<OracleCheckPlanV1>, MigrationAgentRuntimeError> {
        let tasks = self
            .0
            .read()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?;
        let oracle = tasks
            .get(&task_id)
            .and_then(|task| task.oracle.as_ref())
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if proposal.workspace()
            != oracle
                .workspace
                .identity()
                .map_err(MigrationAgentRuntimeError::domain)?
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        Ok(proposal
            .accepted_items()
            .iter()
            .flat_map(|accepted| accepted.plans().iter().cloned())
            .collect())
    }

    fn task(&self, task_id: TaskId) -> Result<RuntimeTaskV1, MigrationAgentRuntimeError> {
        self.0
            .read()
            .map_err(|_| MigrationAgentRuntimeError::StatePoisoned)?
            .get(&task_id)
            .cloned()
            .ok_or(MigrationAgentRuntimeError::UnknownTask(task_id))
    }
}

fn runtime_oracle_dimensions(
    task_id: TaskId,
    oracle: &RuntimeOracleTaskV1,
) -> Result<Vec<OracleDimensionV1>, MigrationAgentRuntimeError> {
    let claims = runtime_oracle_claims(task_id, oracle);
    let mut claim_ids = claims
        .iter()
        .map(OracleClaimV1::identity)
        .collect::<Result<Vec<_>, _>>()
        .map_err(MigrationAgentRuntimeError::domain)?;
    claim_ids.sort_by_key(ContentId::to_wire);
    derive_oracle_dimensions(&claim_ids, &oracle.policy).map_err(MigrationAgentRuntimeError::domain)
}

fn runtime_oracle_claims(task_id: TaskId, oracle: &RuntimeOracleTaskV1) -> Vec<OracleClaimV1> {
    derive_oracle_claims(
        task_id,
        oracle.workspace.admitted_intent(),
        &oracle.admitted_claims,
    )
}

fn runtime_oracle_claim_for_dimension(
    task_id: TaskId,
    oracle: &RuntimeOracleTaskV1,
    dimension: &OracleDimensionV1,
) -> Result<OracleClaimV1, MigrationAgentRuntimeError> {
    exact_oracle_claim_for_dimension(&runtime_oracle_claims(task_id, oracle), dimension)
}

fn exact_oracle_claim_for_dimension(
    claims: &[OracleClaimV1],
    dimension: &OracleDimensionV1,
) -> Result<OracleClaimV1, MigrationAgentRuntimeError> {
    claims
        .iter()
        .find(|claim| {
            claim
                .identity()
                .is_ok_and(|identity| identity == dimension.claim())
        })
        .cloned()
        .ok_or(MigrationAgentRuntimeError::TaskBinding)
}

/// Production executor for role-scoped migration Agent Loops.
pub struct MigrationAgentRuntimeExecutorV1 {
    model: ResolvedRuntimeModel,
    selection: ModelSelection,
    budget: EpisodeBudget,
    max_output_tokens: ModelOutputTokenLimit,
    credential_base: PathBuf,
    events: SqliteEventStore,
    content: SqliteContentStore,
    execution_content: SqliteContentStore,
    materials: MigrationRuntimeMaterialsV1,
    sir_submissions: BTreeMap<AgentLoopId, IntentHypothesisSetProposalV1>,
    whole_portfolio_submissions: BTreeMap<AgentLoopId, OraclePortfolioProposalV1>,
    item_set_submissions: BTreeMap<AgentLoopId, OracleDimensionItemSetProposalV1>,
    item_set_review_submissions: BTreeMap<AgentLoopId, OracleDimensionItemSetReviewV1>,
    item_draft_submissions: BTreeMap<AgentLoopId, OracleItemDraftV1>,
    item_review_submissions: BTreeMap<AgentLoopId, OracleItemReviewV1>,
    coherence_review_submissions: BTreeMap<AgentLoopId, OraclePortfolioCoherenceReviewV1>,
    candidate_submissions: BTreeMap<AgentLoopId, CandidateProposalV1>,
    evidence_worker: Option<(ServerConfig, EvidenceExperimentWorkerConfigV1)>,
}

impl MigrationAgentRuntimeExecutorV1 {
    /// Opens the shared durable model/tool stores used by all role loops in this application.
    ///
    /// # Errors
    ///
    /// Rejects model-selection drift, an output limit outside model capabilities, or storage
    /// initialization failure.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        model: ResolvedRuntimeModel,
        selection: ModelSelection,
        budget: EpisodeBudget,
        max_output_tokens: ModelOutputTokenLimit,
        credential_base: PathBuf,
        event_database: &Path,
        content_database: &Path,
        content_directory: &Path,
        execution_content_database: &Path,
        execution_content_directory: &Path,
        materials: MigrationRuntimeMaterialsV1,
        evidence_worker: Option<(ServerConfig, EvidenceExperimentWorkerConfigV1)>,
    ) -> Result<Self, MigrationAgentRuntimeError> {
        if selection.provider != *model.provider()
            || selection.model != *model.wire_model()
            || selection.deployment != *model.deployment()
            || max_output_tokens > model.capabilities().max_output_tokens()
        {
            return Err(MigrationAgentRuntimeError::ModelBinding);
        }
        Ok(Self {
            model,
            selection,
            budget,
            max_output_tokens,
            credential_base,
            events: SqliteEventStore::open(event_database)
                .map_err(MigrationAgentRuntimeError::domain)?,
            content: SqliteContentStore::open(content_database, content_directory)
                .map_err(MigrationAgentRuntimeError::domain)?,
            execution_content: SqliteContentStore::open(
                execution_content_database,
                execution_content_directory,
            )
            .map_err(MigrationAgentRuntimeError::domain)?,
            materials,
            sir_submissions: BTreeMap::new(),
            whole_portfolio_submissions: BTreeMap::new(),
            item_set_submissions: BTreeMap::new(),
            item_set_review_submissions: BTreeMap::new(),
            item_draft_submissions: BTreeMap::new(),
            item_review_submissions: BTreeMap::new(),
            coherence_review_submissions: BTreeMap::new(),
            candidate_submissions: BTreeMap::new(),
            evidence_worker,
        })
    }

    fn execute_evidence_worker_request(
        &mut self,
        task_id: TaskId,
        request: &cairn_agent::AgentWorkerRequestV1,
    ) -> Result<(), MigrationAgentRuntimeError> {
        let task = self.materials.task(task_id)?;
        if !task.reasoning_decomposition.permits_worker_experiments() {
            return Err(MigrationAgentRuntimeError::UnexpectedExternalEffect(
                "reasoning policy does not authorize proposal evidence experiments",
            ));
        }
        let (server, config) = self.evidence_worker.clone().ok_or(
            MigrationAgentRuntimeError::UnexpectedExternalEffect(
                "proposal evidence Worker is not configured",
            ),
        )?;
        let mut runner = EvidenceExperimentRunnerV1::new(server, config, task.workspace)
            .map_err(MigrationAgentRuntimeError::domain)?;
        cairn_agent::execute_agent_worker_request(
            &mut self.events,
            &mut self.content,
            &mut runner,
            request,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)
    }

    /// Builds the exact model-backed synthesis registration consumed by Oracle item development.
    ///
    /// # Errors
    ///
    /// Returns an error if model identity or strategy registration derivation fails.
    pub fn oracle_strategy_catalog(
        &self,
        policy: &OracleCoveragePolicyV1,
    ) -> Result<OracleStrategyCatalogV1, MigrationAgentRuntimeError> {
        let authorship_model = ContentId::<ModelConfigurationArtifact>::derive(
            &self
                .model
                .canonical_bytes()
                .map_err(MigrationAgentRuntimeError::domain)?,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        let invocation = ContentId::<AgentLoopRuntimeBindingArtifact>::derive(
            &cairn_codec::to_vec(&json!({
                "schema_version": SCHEMA_V1,
                "selection": self.selection,
                "budget": self.budget,
                "max_output_tokens": self.max_output_tokens,
                "role": "migration-oracle-item-developer",
                "hook_profile": "migration-oracle-item-development-hooks",
            }))
            .map_err(MigrationAgentRuntimeError::domain)?,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        let tools = OracleStrategyToolCatalogV1::standard()
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        OracleStrategyCatalogV1::new(vec![
            OracleStrategyRegistrationV1::new(
                OracleStrategyName::new(MODEL_BACKED_SYNTHESIS_STRATEGY)
                    .map_err(MigrationAgentRuntimeError::domain)?,
                OracleStrategyKindV1::ModelBackedSynthesis,
                OracleStrategyExecutorV1::AgentLoop {
                    authorship_model,
                    invocation,
                    tools,
                },
                REGISTERED_STRATEGY_ROLES.to_vec(),
                policy.concerns().to_vec(),
            )
            .map_err(MigrationAgentRuntimeError::domain)?,
        ])
        .map_err(MigrationAgentRuntimeError::domain)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role executor keeps frozen projection, exact gateway, and safe lifecycle logging together"
    )]
    fn execute_oracle_item_discovery(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleDimensionItemDiscoveryAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OracleDimensionItemSetProposalV1>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let oracle = task
            .oracle
            .clone()
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if context.workspace()
            != oracle
                .workspace
                .identity()
                .map_err(MigrationAgentRuntimeError::domain)?
            || context.admitted_intent() != oracle.workspace.admitted_intent()
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let dimensions = runtime_oracle_dimensions(context.task_id(), &oracle)?;
        let dimension = dimensions
            .into_iter()
            .find(|dimension| {
                dimension
                    .identity()
                    .is_ok_and(|identity| identity == context.dimension())
            })
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        let dimension_id = dimension
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let claim = runtime_oracle_claim_for_dimension(context.task_id(), &oracle, &dimension)?;
        let claim_id = claim
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let (previous, review) = match (context.previous_item_set(), context.review_feedback()) {
            (None, None) => (None, None),
            (Some(previous_id), Some(review_id)) => {
                let previous = oracle
                    .item_sets
                    .iter()
                    .find(|proposal| {
                        proposal
                            .identity()
                            .is_ok_and(|identity| identity == previous_id)
                    })
                    .cloned()
                    .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
                let review = oracle
                    .item_set_reviews
                    .iter()
                    .find(|review| {
                        review
                            .identity()
                            .is_ok_and(|identity| identity == review_id)
                    })
                    .cloned()
                    .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
                review
                    .validate_against(&previous)
                    .map_err(MigrationAgentRuntimeError::domain)?;
                if previous.dimension() != dimension_id
                    || !matches!(
                        review.decision(),
                        OracleDimensionItemSetReviewDecisionV1::NeedsRevision { .. }
                    )
                {
                    return Err(MigrationAgentRuntimeError::TaskBinding);
                }
                (Some(previous), Some(review))
            }
            _ => return Err(MigrationAgentRuntimeError::TaskBinding),
        };
        if previous.as_ref().is_some_and(|proposal| {
            proposal.revision().get() >= oracle.workspace.budget().item_discovery_revisions.get()
        }) {
            return Err(
                MigrationAgentRuntimeError::OracleItemDiscoveryRevisionBudgetExhausted {
                    dimension: dimension_id,
                    limit: oracle.workspace.budget().item_discovery_revisions,
                },
            );
        }
        archive_exact(&mut self.content, dimension_id, &dimension)?;
        let tools =
            exposed_native_tools(access, &oracle_item_discovery_native_tools(task.limits)?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "workspace_id": context.workspace(),
            "admitted_intent_id": context.admitted_intent(),
            "dimension_id": dimension_id,
            "claim_id": claim_id,
            "previous_item_set_id": context.previous_item_set(),
            "review_feedback_id": context.review_feedback(),
            "task_artifacts": task.workspace.bundle().artifacts(),
            "knowledge_snapshot": {"kind":"empty"},
        });
        let projection = archive_role_projection(
            &mut self.content,
            ORACLE_ITEM_DISCOVERY_INSTRUCTION,
            &tools,
            &model_context,
            format!(
                "Discover independently reviewable items for exact Oracle dimension {dimension_id}."
            ),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    ORACLE_ITEM_DISCOVERY_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "oracle_item_discovery_step_started",
            task_id = %context.task_id(),
            loop_id = %checkpoint.start().loop_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            dimension_id = %dimension_id,
            "Oracle item discovery model/tool step started"
        );
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = OracleDimensionItemDiscoveryGateway {
            task_workspace: task.workspace,
            limits: task.limits,
            dimension,
            claim,
            previous,
            review,
            accepted: self.item_set_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.item_set_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let proposal = match resolve_role_submission(
                    gateway.accepted,
                    completion.reason,
                    "Oracle dimension items",
                )? {
                    RuntimeRoleSubmissionV1::Submitted(proposal) => proposal,
                    RuntimeRoleSubmissionV1::Exhausted(reason) => {
                        return Ok(AgentLoopStepExecutionV1::Observed(
                            MigrationRoleStepObservationV1::Exhausted(reason),
                        ));
                    }
                };
                for item in proposal.items() {
                    archive_exact(
                        &mut self.content,
                        item.identity()
                            .map_err(MigrationAgentRuntimeError::domain)?,
                        item,
                    )?;
                }
                let proposal_id = proposal
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?;
                archive_exact(&mut self.content, proposal_id, &proposal)?;
                self.materials
                    .record_oracle_item_set(context.task_id(), &proposal)?;
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "oracle_item_discovery_episode_completed",
                    task_id = %context.task_id(),
                    episode_id = %episode_id,
                    dimension_id = %dimension_id,
                    proposal_id = %proposal_id,
                    item_count = proposal.items().len(),
                    steps_started = completion.steps_started,
                    "Oracle item discovery episode completed"
                );
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(proposal),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                if let Some(submission) = gateway.accepted {
                    self.item_set_submissions.insert(loop_id, submission);
                }
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role executor keeps exact projection, episode, gateway, archive, and safe-log bindings together"
    )]
    fn execute_oracle_item_set_review(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleDimensionItemSetReviewerAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OracleDimensionItemSetReviewV1>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let oracle = task
            .oracle
            .clone()
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if context.admitted_intent() != oracle.workspace.admitted_intent() {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let proposal = oracle
            .item_sets
            .iter()
            .find(|proposal| {
                proposal
                    .identity()
                    .is_ok_and(|identity| identity == context.proposal())
            })
            .cloned()
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        if proposal.dimension() != context.dimension() {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let dimension = runtime_oracle_dimensions(context.task_id(), &oracle)?
            .into_iter()
            .find(|dimension| {
                dimension
                    .identity()
                    .is_ok_and(|identity| identity == context.dimension())
            })
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        let claim = runtime_oracle_claim_for_dimension(context.task_id(), &oracle, &dimension)?;
        let claim_id = claim
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let tools =
            exposed_native_tools(access, &oracle_item_set_reviewer_native_tools(task.limits)?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "admitted_intent_id": context.admitted_intent(),
            "dimension_id": context.dimension(),
            "claim_id": claim_id,
            "proposal_id": context.proposal(),
            "revision": proposal.revision().get(),
            "task_artifacts": task.workspace.bundle().artifacts(),
        });
        let projection = archive_role_projection(
            &mut self.content,
            ORACLE_ITEM_SET_REVIEW_INSTRUCTION,
            &tools,
            &model_context,
            format!(
                "Review exact Oracle item-set proposal {}.",
                context.proposal()
            ),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    ORACLE_ITEM_SET_REVIEW_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "oracle_item_set_review_step_started",
            task_id = %context.task_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            dimension_id = %context.dimension(),
            proposal_id = %context.proposal(),
            "Oracle item-set Review model/tool step started"
        );
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = OracleDimensionItemSetReviewerGateway {
            task_workspace: task.workspace,
            limits: task.limits,
            dimension,
            claim,
            proposal,
            accepted: self.item_set_review_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.item_set_review_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let review = match resolve_role_submission(
                    gateway.accepted,
                    completion.reason,
                    "Oracle item-set review",
                )? {
                    RuntimeRoleSubmissionV1::Submitted(review) => review,
                    RuntimeRoleSubmissionV1::Exhausted(reason) => {
                        return Ok(AgentLoopStepExecutionV1::Observed(
                            MigrationRoleStepObservationV1::Exhausted(reason),
                        ));
                    }
                };
                let review_id = review
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?;
                archive_exact(&mut self.content, review_id, &review)?;
                self.materials
                    .record_oracle_item_set_review(context.task_id(), &review)?;
                let (decision, finding_count) = match review.decision() {
                    OracleDimensionItemSetReviewDecisionV1::Approved => ("approved", 0),
                    OracleDimensionItemSetReviewDecisionV1::NeedsRevision { findings } => {
                        ("needs-revision", findings.len())
                    }
                };
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "oracle_item_set_review_episode_completed",
                    task_id = %context.task_id(),
                    episode_id = %episode_id,
                    dimension_id = %context.dimension(),
                    proposal_id = %context.proposal(),
                    review_id = %review_id,
                    decision,
                    finding_count,
                    steps_started = completion.steps_started,
                    "Oracle item-set Review episode completed"
                );
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(review),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                if let Some(submission) = gateway.accepted {
                    self.item_set_review_submissions.insert(loop_id, submission);
                }
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role executor keeps material binding, frozen projection, and episode outcome handling together"
    )]
    fn execute_candidate_exploration(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &CandidateExplorationAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<Option<CandidateProposalV1>>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let candidate = task
            .candidate
            .clone()
            .ok_or(MigrationAgentRuntimeError::MissingCandidateMaterials)?;
        let contract_id = candidate
            .contract
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let workspace_id = candidate
            .workspace
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if context.oracle_contract() != contract_id
            || context.candidate_workspace() != workspace_id
            || context.admitted_intent() != candidate.workspace.admitted_intent()
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let tools =
            exposed_native_tools(access, &candidate_exploration_native_tools(task.limits)?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "candidate_workspace_id": workspace_id,
            "oracle_contract_id": contract_id,
            "admitted_intent_id": context.admitted_intent(),
            "admitted_oracle_id": context.admitted_oracle(),
            "task_artifacts": task.workspace.bundle().artifacts(),
            "knowledge_snapshot": {"kind":"empty"},
        });
        let projection = archive_role_projection(
            &mut self.content,
            CANDIDATE_EXPLORATION_INSTRUCTION,
            &tools,
            &model_context,
            "Write the Ascend C implementation this task's admitted claims require.".to_owned(),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let model_configuration = ContentId::<AgentResolvedRuntimeModelArtifact>::derive(
            &self
                .model
                .canonical_bytes()
                .map_err(MigrationAgentRuntimeError::domain)?,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    CANDIDATE_EXPLORATION_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "candidate_exploration_step_started",
            task_id = %context.task_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            oracle_contract_id = %contract_id,
            "Candidate exploration model/tool step started"
        );
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = CandidateExplorationGateway {
            task_workspace: task.workspace,
            limits: task.limits,
            contract: candidate.contract,
            contract_id,
            episode_id,
            model_configuration,
            accepted: self.candidate_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.candidate_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let proposal = self.settle_candidate_submission(
                    gateway.accepted,
                    completion.reason,
                    context.task_id(),
                    episode_id,
                )?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(proposal),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role executor keeps material binding, frozen projection, and episode outcome handling together"
    )]
    fn execute_candidate_revision(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &CandidateRevisionAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<Option<CandidateProposalV1>>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let candidate = task
            .candidate
            .clone()
            .ok_or(MigrationAgentRuntimeError::MissingCandidateMaterials)?;
        let contract_id = candidate
            .contract
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let workspace_id = candidate
            .workspace
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if context.oracle_contract() != contract_id || context.candidate_workspace() != workspace_id
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let parent: CandidateProposalV1 = load_exact(&self.content, context.parent())?;
        if parent
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?
            != context.parent()
        {
            return Err(MigrationAgentRuntimeError::ArtifactBinding);
        }
        let receipt: cairn_execution::ExecutionReceipt =
            load_exact(&self.execution_content, context.receipt())?;
        let diagnostic = CandidateBuildDiagnosticV1 {
            receipt: context.receipt(),
            outcome: receipt.outcome(),
            exit_code: receipt.exit_code(),
            stdout: read_control_diagnostic_artifact(&self.execution_content, receipt.stdout_id())?,
            stderr: read_control_diagnostic_artifact(&self.execution_content, receipt.stderr_id())?,
        };
        let tools = exposed_native_tools(access, &candidate_revision_native_tools(task.limits)?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "candidate_workspace_id": workspace_id,
            "oracle_contract_id": contract_id,
            "parent_proposal_id": context.parent(),
            "build_receipt_id": context.receipt(),
            "iteration": context.iteration(),
            "build_attempts_remaining": context.remaining(),
            "controller_notice": context.notice(),
            "task_artifacts": task.workspace.bundle().artifacts(),
            "knowledge_snapshot": {"kind":"empty"},
        });
        let projection = archive_role_projection(
            &mut self.content,
            CANDIDATE_REVISION_INSTRUCTION,
            &tools,
            &model_context,
            "Revise the implementation the last build refused.".to_owned(),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let model_configuration = ContentId::<AgentResolvedRuntimeModelArtifact>::derive(
            &self
                .model
                .canonical_bytes()
                .map_err(MigrationAgentRuntimeError::domain)?,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    CANDIDATE_REVISION_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "candidate_revision_step_started",
            task_id = %context.task_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            iteration = context.iteration().get(),
            remaining = context.remaining().get(),
            has_notice = context.notice().is_some(),
            "Candidate revision model/tool step started"
        );
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = CandidateRevisionGateway {
            task_workspace: task.workspace,
            limits: task.limits,
            contract: candidate.contract,
            contract_id,
            parent_id: context.parent(),
            parent,
            diagnostic,
            notice: context.notice(),
            remaining: context.remaining(),
            episode_id,
            model_configuration,
            accepted: self.candidate_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.candidate_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let proposal = self.settle_candidate_submission(
                    gateway.accepted,
                    completion.reason,
                    context.task_id(),
                    episode_id,
                )?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(proposal),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    /// Turns one finished proposal episode into what the search loop counts.
    ///
    /// An episode that ended without submitting is `None`, not an error. The Controller records it
    /// as a failed attempt and stops only after a run of them, because a reasoning model can spend
    /// its whole output budget before it ever calls the submit tool.
    fn settle_candidate_submission(
        &mut self,
        accepted: Option<CandidateProposalV1>,
        reason: cairn_agent::EpisodeCompletionReason,
        task_id: TaskId,
        episode_id: cairn_protocol::EpisodeId,
    ) -> Result<Option<CandidateProposalV1>, MigrationAgentRuntimeError> {
        let proposal = match resolve_role_submission(accepted, reason, "Candidate proposal") {
            Ok(RuntimeRoleSubmissionV1::Submitted(proposal)) => proposal,
            Ok(RuntimeRoleSubmissionV1::Exhausted(_))
            | Err(MigrationAgentRuntimeError::MissingSubmission(_)) => {
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "candidate_episode_submitted_nothing",
                    task_id = %task_id,
                    episode_id = %episode_id,
                    "Candidate episode ended without a proposal"
                );
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let proposal_id = proposal
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        archive_exact(&mut self.content, proposal_id, &proposal)?;
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "candidate_proposal_submitted",
            task_id = %task_id,
            episode_id = %episode_id,
            proposal_id = %proposal_id,
            file_count = proposal.submission().files().len(),
            "Candidate episode produced one exact proposal"
        );
        Ok(Some(proposal))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role executor keeps exact revision lineage, frozen projection, and safe lifecycle logging together"
    )]
    fn execute_oracle_item_development(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleItemDeveloperAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OracleItemDraftV1>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let oracle = task
            .oracle
            .clone()
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if context.workspace()
            != oracle
                .workspace
                .identity()
                .map_err(MigrationAgentRuntimeError::domain)?
            || context.admitted_intent() != oracle.workspace.admitted_intent()
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let item = oracle
            .item_sets
            .iter()
            .flat_map(OracleDimensionItemSetProposalV1::items)
            .find(|item| {
                item.identity()
                    .is_ok_and(|identity| identity == context.item())
            })
            .cloned()
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        let (previous, review, coherence, admission) = match (
            context.previous_draft(),
            context.review_feedback(),
            context.coherence_feedback(),
            context.admission_feedback(),
        ) {
            (None, None, None, None) => (None, None, None, None),
            (Some(draft_id), review_id, coherence_id, admission_id)
                if review_id.is_some() || coherence_id.is_some() || admission_id.is_some() =>
            {
                let draft = oracle
                    .item_drafts
                    .iter()
                    .find(|draft| draft.identity().is_ok_and(|identity| identity == draft_id))
                    .cloned()
                    .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
                let review = review_id
                    .map(|review_id| {
                        oracle
                            .item_reviews
                            .iter()
                            .find(|review| {
                                review
                                    .identity()
                                    .is_ok_and(|identity| identity == review_id)
                            })
                            .cloned()
                            .ok_or(MigrationAgentRuntimeError::TaskBinding)
                    })
                    .transpose()?;
                if let Some(review) = &review {
                    review
                        .validate_against(&draft)
                        .map_err(MigrationAgentRuntimeError::domain)?;
                    if !matches!(
                        review.decision(),
                        OracleItemReviewDecisionV1::NeedsRevision { .. }
                    ) {
                        return Err(MigrationAgentRuntimeError::TaskBinding);
                    }
                }
                let admission = admission_id
                    .map(|request_id| {
                        oracle
                            .revision_requests
                            .iter()
                            .find(|request| {
                                request
                                    .identity()
                                    .is_ok_and(|identity| identity == request_id)
                            })
                            .cloned()
                            .ok_or(MigrationAgentRuntimeError::TaskBinding)
                    })
                    .transpose()?;
                let coherence = coherence_id
                    .map(|review_id| {
                        oracle
                            .coherence_reviews
                            .iter()
                            .find(|review| {
                                review
                                    .identity()
                                    .is_ok_and(|identity| identity == review_id)
                            })
                            .cloned()
                            .ok_or(MigrationAgentRuntimeError::TaskBinding)
                    })
                    .transpose()?;
                if let Some(coherence) = &coherence {
                    let portfolio = oracle
                        .portfolios
                        .iter()
                        .find(|portfolio| {
                            portfolio
                                .identity()
                                .is_ok_and(|identity| identity == coherence.portfolio())
                        })
                        .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
                    coherence
                        .validate_against(portfolio)
                        .map_err(MigrationAgentRuntimeError::domain)?;
                    if !matches!(
                        coherence.decision(),
                        OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings }
                            if findings.iter().any(|finding| {
                                finding.affected_items().items().contains(&context.item())
                            })
                    ) || !portfolio.accepted_items().iter().any(|accepted| {
                        accepted
                            .draft()
                            .identity()
                            .is_ok_and(|identity| identity == draft_id)
                    }) {
                        return Err(MigrationAgentRuntimeError::TaskBinding);
                    }
                }
                if draft
                    .item()
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?
                    != context.item()
                    || admission.as_ref().is_some_and(|request| {
                        !oracle_admission_targets_item(request, context.item())
                    })
                {
                    return Err(MigrationAgentRuntimeError::TaskBinding);
                }
                (Some(draft), review, coherence, admission)
            }
            _ => return Err(MigrationAgentRuntimeError::TaskBinding),
        };
        let dimension = runtime_oracle_dimensions(context.task_id(), &oracle)?
            .into_iter()
            .find(|dimension| {
                dimension
                    .identity()
                    .is_ok_and(|identity| identity == item.dimension())
            })
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        let claim = runtime_oracle_claim_for_dimension(context.task_id(), &oracle, &dimension)?;
        let claim_id = claim
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        if previous.as_ref().is_some_and(|draft| {
            draft.revision().get() >= oracle.workspace.budget().item_revisions.get()
        }) {
            return Err(
                MigrationAgentRuntimeError::OracleItemRevisionBudgetExhausted {
                    item: context.item(),
                    limit: oracle.workspace.budget().item_revisions,
                },
            );
        }
        let run = OracleStrategyRunV1::new(
            context.workspace(),
            &dimension,
            OracleStrategyName::new(MODEL_BACKED_SYNTHESIS_STRATEGY)
                .map_err(MigrationAgentRuntimeError::domain)?,
            &oracle.catalog,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        let run_id = run.identity().map_err(MigrationAgentRuntimeError::domain)?;
        archive_exact(&mut self.content, run_id, &run)?;
        let tools = exposed_native_tools(
            access,
            &oracle_item_developer_native_tools(context.admitted_intent(), task.limits)?,
        )?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "workspace_id": context.workspace(),
            "admitted_intent_id": context.admitted_intent(),
            "item_id": context.item(),
            "dimension_id": item.dimension(),
            "claim_id": claim_id,
            "previous_draft_id": context.previous_draft(),
            "review_feedback_id": context.review_feedback(),
            "coherence_feedback_id": context.coherence_feedback(),
            "admission_feedback_id": context.admission_feedback(),
            "task_artifacts": task.workspace.bundle().artifacts(),
            "knowledge_snapshot": {"kind":"empty"},
        });
        let projection = archive_role_projection(
            &mut self.content,
            ORACLE_ITEM_DEVELOPMENT_INSTRUCTION,
            &tools,
            &model_context,
            format!("Develop exact Oracle item {}.", context.item()),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    ORACLE_ITEM_DEVELOPMENT_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "oracle_item_development_step_started",
            task_id = %context.task_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            item_id = %context.item(),
            has_review_feedback = review.is_some(),
            has_coherence_feedback = coherence.is_some(),
            has_admission_feedback = admission.is_some(),
            "Oracle item development model/tool step started"
        );
        let control_diagnostics = admission
            .as_ref()
            .map(|request| {
                load_oracle_control_diagnostics(&self.execution_content, request, context.item())
            })
            .transpose()?
            .unwrap_or_default();
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = OracleItemDeveloperGateway {
            task_workspace: task.workspace,
            limits: task.limits,
            oracle_workspace: oracle.workspace,
            item,
            dimension,
            claim,
            previous,
            review,
            coherence,
            admission,
            control_diagnostics,
            run,
            accepted: self.item_draft_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.item_draft_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let draft = match resolve_role_submission(
                    gateway.accepted,
                    completion.reason,
                    "Oracle item draft",
                )? {
                    RuntimeRoleSubmissionV1::Submitted(draft) => draft,
                    RuntimeRoleSubmissionV1::Exhausted(reason) => {
                        return Ok(AgentLoopStepExecutionV1::Observed(
                            MigrationRoleStepObservationV1::Exhausted(reason),
                        ));
                    }
                };
                for plan in draft.plans() {
                    archive_exact(
                        &mut self.content,
                        plan.identity()
                            .map_err(MigrationAgentRuntimeError::domain)?,
                        plan,
                    )?;
                }
                let draft_id = draft
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?;
                archive_exact(&mut self.content, draft_id, &draft)?;
                self.materials
                    .record_oracle_item_draft(context.task_id(), &draft)?;
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "oracle_item_development_episode_completed",
                    task_id = %context.task_id(),
                    episode_id = %episode_id,
                    item_id = %context.item(),
                    draft_id = %draft_id,
                    revision = draft.revision().get(),
                    plan_count = draft.plans().len(),
                    steps_started = completion.steps_started,
                    "Oracle item development episode completed"
                );
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(draft),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                if let Some(submission) = gateway.accepted {
                    self.item_draft_submissions.insert(loop_id, submission);
                }
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role executor keeps exact draft binding, frozen projection, and safe lifecycle logging together"
    )]
    fn execute_oracle_item_review(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleItemReviewerAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OracleItemReviewV1>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let oracle = task
            .oracle
            .clone()
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if context.admitted_intent() != oracle.workspace.admitted_intent() {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let draft = oracle
            .item_drafts
            .iter()
            .find(|draft| {
                draft
                    .identity()
                    .is_ok_and(|identity| identity == context.draft())
            })
            .cloned()
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        if draft
            .item()
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?
            != context.item()
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let dimension = runtime_oracle_dimensions(context.task_id(), &oracle)?
            .into_iter()
            .find(|dimension| {
                dimension
                    .identity()
                    .is_ok_and(|identity| identity == draft.item().dimension())
            })
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        let claim = runtime_oracle_claim_for_dimension(context.task_id(), &oracle, &dimension)?;
        let claim_id = claim
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let tools = exposed_native_tools(access, &oracle_item_reviewer_native_tools(task.limits)?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "admitted_intent_id": context.admitted_intent(),
            "item_id": context.item(),
            "dimension_id": draft.item().dimension(),
            "claim_id": claim_id,
            "draft_id": context.draft(),
            "revision": draft.revision().get(),
            "task_artifacts": task.workspace.bundle().artifacts(),
        });
        let projection = archive_role_projection(
            &mut self.content,
            ORACLE_ITEM_REVIEW_INSTRUCTION,
            &tools,
            &model_context,
            format!("Review exact Oracle item draft {}.", context.draft()),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    ORACLE_ITEM_REVIEW_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "oracle_item_review_step_started",
            task_id = %context.task_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            item_id = %context.item(),
            draft_id = %context.draft(),
            "Oracle item Review model/tool step started"
        );
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = OracleItemReviewerGateway {
            task_workspace: task.workspace,
            limits: task.limits,
            dimension,
            claim,
            draft,
            accepted: self.item_review_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.item_review_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let review = match resolve_role_submission(
                    gateway.accepted,
                    completion.reason,
                    "Oracle item review",
                )? {
                    RuntimeRoleSubmissionV1::Submitted(review) => review,
                    RuntimeRoleSubmissionV1::Exhausted(reason) => {
                        return Ok(AgentLoopStepExecutionV1::Observed(
                            MigrationRoleStepObservationV1::Exhausted(reason),
                        ));
                    }
                };
                let review_id = review
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?;
                archive_exact(&mut self.content, review_id, &review)?;
                self.materials
                    .record_oracle_item_review(context.task_id(), &review)?;
                let (decision, finding_count) = match review.decision() {
                    OracleItemReviewDecisionV1::Approved => ("approved", 0),
                    OracleItemReviewDecisionV1::NeedsRevision { findings } => {
                        ("needs-revision", findings.len())
                    }
                };
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "oracle_item_review_episode_completed",
                    task_id = %context.task_id(),
                    episode_id = %episode_id,
                    item_id = %context.item(),
                    draft_id = %context.draft(),
                    review_id = %review_id,
                    decision,
                    finding_count,
                    steps_started = completion.steps_started,
                    "Oracle item Review episode completed"
                );
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(review),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                if let Some(submission) = gateway.accepted {
                    self.item_review_submissions.insert(loop_id, submission);
                }
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role executor keeps exact portfolio binding, frozen projection, and safe lifecycle logging together"
    )]
    fn execute_oracle_portfolio_coherence_review(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OraclePortfolioCoherenceReviewerAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OraclePortfolioCoherenceReviewV1>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let oracle = task
            .oracle
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if context.admitted_intent() != oracle.workspace.admitted_intent() {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let portfolio = oracle
            .portfolios
            .iter()
            .find(|portfolio| {
                portfolio
                    .identity()
                    .is_ok_and(|identity| identity == context.portfolio())
            })
            .cloned()
            .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
        let claims = runtime_oracle_claims(context.task_id(), &oracle);
        let dimensions = runtime_oracle_dimensions(context.task_id(), &oracle)?;
        let tools =
            exposed_native_tools(access, &oracle_portfolio_coherence_reviewer_native_tools()?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "admitted_intent_id": context.admitted_intent(),
            "portfolio_id": context.portfolio(),
            "item_count": portfolio.accepted_items().len(),
            "claim_count": claims.len(),
            "dimension_count": dimensions.len(),
        });
        let projection = archive_role_projection(
            &mut self.content,
            ORACLE_PORTFOLIO_COHERENCE_REVIEW_INSTRUCTION,
            &tools,
            &model_context,
            format!(
                "Review cross-item coherence of exact Oracle portfolio {}.",
                context.portfolio()
            ),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    ORACLE_PORTFOLIO_COHERENCE_REVIEW_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "oracle_portfolio_coherence_review_step_started",
            task_id = %context.task_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            portfolio_id = %context.portfolio(),
            "Oracle portfolio coherence Review model/tool step started"
        );
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = OraclePortfolioCoherenceReviewerGateway {
            claims,
            dimensions,
            portfolio,
            accepted: self.coherence_review_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.coherence_review_submissions
                        .insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let review = match resolve_role_submission(
                    gateway.accepted,
                    completion.reason,
                    "Oracle portfolio coherence review",
                )? {
                    RuntimeRoleSubmissionV1::Submitted(review) => review,
                    RuntimeRoleSubmissionV1::Exhausted(reason) => {
                        return Ok(AgentLoopStepExecutionV1::Observed(
                            MigrationRoleStepObservationV1::Exhausted(reason),
                        ));
                    }
                };
                let review_id = review
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?;
                archive_exact(&mut self.content, review_id, &review)?;
                self.materials
                    .record_oracle_coherence_review(context.task_id(), &review)?;
                let (decision, finding_count) = match review.decision() {
                    OraclePortfolioCoherenceDecisionV1::Approved => ("approved", 0),
                    OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings } => {
                        ("needs-revision", findings.len())
                    }
                };
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "oracle_portfolio_coherence_review_episode_completed",
                    task_id = %context.task_id(),
                    episode_id = %episode_id,
                    portfolio_id = %context.portfolio(),
                    review_id = %review_id,
                    decision,
                    finding_count,
                    steps_started = completion.steps_started,
                    "Oracle portfolio coherence Review episode completed"
                );
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(review),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                if let Some(submission) = gateway.accepted {
                    self.coherence_review_submissions
                        .insert(loop_id, submission);
                }
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the role step keeps its frozen model, prompt, tool, and typed submission bindings visible"
    )]
    fn execute_sir(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &SirAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        if task
            .workspace
            .bundle()
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?
            != context.task_bundle()
            || task
                .recovery_input
                .identity()
                .map_err(MigrationAgentRuntimeError::domain)?
                != context.recovery_input()
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let tools = exposed_native_tools(access, &sir_native_tools(task.limits)?)?;
        let projection = archive_sir_projection(&mut self.content, &task, &tools)?;
        let episode_id = checkpoint.start().episode_id();
        let model_configuration = ContentId::<AgentResolvedRuntimeModelArtifact>::derive(
            &self
                .model
                .canonical_bytes()
                .map_err(MigrationAgentRuntimeError::domain)?,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        let codec = NativeProtocolCodec::from_config(self.model.protocol())
            .map_err(MigrationAgentRuntimeError::domain)?;
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(SIR_INSTRUCTION, task.reasoning_decomposition),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        tracing::info!(
            target: "cairn.migration.agent-runtime",
            event = "migration_role_step_started",
            task_id = %context.task_id(),
            loop_id = %checkpoint.start().loop_id(),
            episode_id = %episode_id,
            step_ordinal = checkpoint.steps_started(),
            role = %checkpoint.start().role(),
            "migration role model/tool step started"
        );
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = SirGateway {
            workspace: task.workspace,
            recovery_input: task.recovery_input,
            limits: task.limits,
            episode_id,
            model_configuration,
            accepted: self.sir_submissions.remove(&loop_id),
        };
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.sir_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let proposal =
                    match resolve_role_submission(gateway.accepted, completion.reason, "SIR")? {
                        RuntimeRoleSubmissionV1::Submitted(proposal) => proposal,
                        RuntimeRoleSubmissionV1::Exhausted(reason) => {
                            return Ok(AgentLoopStepExecutionV1::Observed(
                                MigrationRoleStepObservationV1::Exhausted(reason),
                            ));
                        }
                    };
                let proposal_id = proposal
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?;
                archive_exact(&mut self.content, proposal_id, &proposal)?;
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "migration_role_episode_completed",
                    task_id = %context.task_id(),
                    loop_id = %checkpoint.start().loop_id(),
                    episode_id = %episode_id,
                    role = %checkpoint.start().role(),
                    steps_started = completion.steps_started,
                    proposal_id = %proposal_id,
                    "migration role model episode completed with a typed submission"
                );
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(proposal),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                if let Some(submission) = gateway.accepted {
                    self.sir_submissions.insert(loop_id, submission);
                }
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the minimal-decomposition executor keeps one exact whole-portfolio authority boundary visible"
    )]
    fn execute_oracle_whole_portfolio(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleWholePortfolioAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> Result<
        AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OraclePortfolioProposalV1>>,
        MigrationAgentRuntimeError,
    > {
        let task = self.materials.task(context.task_id())?;
        let oracle = task
            .oracle
            .clone()
            .ok_or(MigrationAgentRuntimeError::MissingOracleMaterials)?;
        if context.admitted_intent() != oracle.workspace.admitted_intent()
            || context.workspace()
                != oracle
                    .workspace
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let dimensions = runtime_oracle_dimensions(context.task_id(), &oracle)?;
        let authority = OracleWholePortfolioProposalAuthorityV1::new(
            &oracle.workspace,
            dimensions
                .iter()
                .map(OracleDimensionV1::identity)
                .collect::<Result<Vec<_>, _>>()
                .map_err(MigrationAgentRuntimeError::domain)?,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        if authority
            .identity()
            .map_err(MigrationAgentRuntimeError::domain)?
            != context.authority()
        {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let previous = context
            .previous_portfolio()
            .map(|identity| {
                oracle
                    .portfolios
                    .iter()
                    .find(|portfolio| {
                        portfolio
                            .identity()
                            .is_ok_and(|existing| existing == identity)
                    })
                    .cloned()
                    .ok_or(MigrationAgentRuntimeError::TaskBinding)
            })
            .transpose()?;
        let admission = context
            .admission_feedback()
            .map(|identity| {
                oracle
                    .revision_requests
                    .iter()
                    .find(|request| {
                        request
                            .identity()
                            .is_ok_and(|existing| existing == identity)
                    })
                    .cloned()
                    .ok_or(MigrationAgentRuntimeError::TaskBinding)
            })
            .transpose()?;
        if previous.is_some() != admission.is_some() {
            return Err(MigrationAgentRuntimeError::TaskBinding);
        }
        let claims = runtime_oracle_claims(context.task_id(), &oracle);
        let tools = exposed_native_tools(
            access,
            &oracle_whole_portfolio_native_tools(context.admitted_intent(), task.limits)?,
        )?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "workspace_id": context.workspace(),
            "admitted_intent_id": context.admitted_intent(),
            "authority_id": context.authority(),
            "dimension_count": dimensions.len(),
            "previous_portfolio_id": context.previous_portfolio(),
            "admission_feedback_id": context.admission_feedback(),
            "task_artifacts": task.workspace.bundle().artifacts(),
            "target_context": task.recovery_input,
            "knowledge_snapshot": {"kind":"empty"},
        });
        let projection = archive_role_projection(
            &mut self.content,
            ORACLE_WHOLE_PORTFOLIO_INSTRUCTION,
            &tools,
            &model_context,
            format!(
                "Propose the complete Oracle portfolio under exact authority {}.",
                context.authority()
            ),
        )?;
        let episode_id = checkpoint.start().episode_id();
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: context.task_id(),
            episode_id,
            role: checkpoint.start().role().clone(),
            selection: self.selection.clone(),
            budget: self.budget.clone(),
            native_spec: NativeRequestSpec {
                wire_model: self.selection.model.clone(),
                instructions: role_instruction(
                    ORACLE_WHOLE_PORTFOLIO_INSTRUCTION,
                    task.reasoning_decomposition,
                ),
                tools: tools.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            user_text: projection.user_text,
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.history,
            context: projection.context,
            policy: projection.policy,
            capability_grant: capability_grant(access, &tools)?,
        };
        let loop_id = checkpoint.start().loop_id();
        let mut gateway = OracleWholePortfolioGateway {
            task_workspace: task.workspace,
            limits: task.limits,
            recovery_input: task.recovery_input,
            workspace: oracle.workspace,
            dimensions,
            claims,
            catalog: oracle.catalog,
            authority,
            previous,
            admission,
            runs: Vec::new(),
            accepted: self.whole_portfolio_submissions.remove(&loop_id),
        };
        let mut transport = HttpModelTransport::new(&self.model, &self.credential_base)
            .map_err(MigrationAgentRuntimeError::domain)?;
        let outcome = drive_agent_episode_step(
            &mut self.events,
            &mut self.content,
            &mut transport,
            NativeProtocolCodec::from_config(self.model.protocol())
                .map_err(MigrationAgentRuntimeError::domain)?,
            &frozen,
            &mut gateway,
        )
        .map_err(MigrationAgentRuntimeError::episode_driver)?;
        match outcome {
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Continue => {
                if let Some(submission) = gateway.accepted {
                    self.whole_portfolio_submissions.insert(loop_id, submission);
                }
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::Complete(completion) => {
                let portfolio = match resolve_role_submission(
                    gateway.accepted,
                    completion.reason,
                    "Oracle whole portfolio",
                )? {
                    RuntimeRoleSubmissionV1::Submitted(portfolio) => portfolio,
                    RuntimeRoleSubmissionV1::Exhausted(reason) => {
                        return Ok(AgentLoopStepExecutionV1::Observed(
                            MigrationRoleStepObservationV1::Exhausted(reason),
                        ));
                    }
                };
                archive_exact(
                    &mut self.content,
                    gateway
                        .authority
                        .identity()
                        .map_err(MigrationAgentRuntimeError::domain)?,
                    &gateway.authority,
                )?;
                for run in &gateway.runs {
                    archive_exact(
                        &mut self.content,
                        run.identity().map_err(MigrationAgentRuntimeError::domain)?,
                        run,
                    )?;
                }
                for accepted in portfolio.accepted_items() {
                    for plan in accepted.plans() {
                        archive_exact(
                            &mut self.content,
                            plan.identity()
                                .map_err(MigrationAgentRuntimeError::domain)?,
                            plan,
                        )?;
                    }
                    archive_exact(
                        &mut self.content,
                        accepted
                            .draft()
                            .identity()
                            .map_err(MigrationAgentRuntimeError::domain)?,
                        accepted.draft(),
                    )?;
                    archive_exact(
                        &mut self.content,
                        accepted
                            .identity()
                            .map_err(MigrationAgentRuntimeError::domain)?,
                        accepted,
                    )?;
                    self.materials
                        .record_oracle_item_draft(context.task_id(), accepted.draft())?;
                }
                let portfolio_id = portfolio
                    .identity()
                    .map_err(MigrationAgentRuntimeError::domain)?;
                archive_exact(&mut self.content, portfolio_id, &portfolio)?;
                self.materials
                    .record_oracle_portfolio(context.task_id(), &portfolio)?;
                tracing::info!(
                    target: "cairn.migration.agent-runtime",
                    event = "oracle_whole_portfolio_episode_completed",
                    task_id = %context.task_id(),
                    episode_id = %episode_id,
                    authority_id = %context.authority(),
                    portfolio_id = %portfolio_id,
                    dimension_count = portfolio.entries().len(),
                    item_count = portfolio.accepted_items().len(),
                    steps_started = completion.steps_started,
                    "minimal-decomposition Oracle whole-portfolio episode completed"
                );
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Complete(portfolio),
                ))
            }
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) => {
                if let Some(submission) = gateway.accepted {
                    self.whole_portfolio_submissions.insert(loop_id, submission);
                }
                self.execute_evidence_worker_request(context.task_id(), &request)?;
                Ok(AgentLoopStepExecutionV1::Observed(
                    MigrationRoleStepObservationV1::Continue,
                ))
            }
        }
    }
}

impl
    AgentLoopStepExecutor<
        SirAgentContextV1,
        MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &SirAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>>,
            Self::Error,
        >,
    > + Send {
        ready(tokio::task::block_in_place(|| {
            self.execute_sir(checkpoint, context, access)
        }))
    }
}

impl
    AgentLoopStepExecutor<
        OracleWholePortfolioAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleWholePortfolioAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OraclePortfolioProposalV1>>,
            Self::Error,
        >,
    > + Send {
        ready(tokio::task::block_in_place(|| {
            self.execute_oracle_whole_portfolio(checkpoint, context, access)
        }))
    }
}

impl
    AgentLoopStepExecutor<
        OracleDimensionItemDiscoveryAgentContextV1,
        MigrationRoleStepObservationV1<OracleDimensionItemSetProposalV1>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleDimensionItemDiscoveryAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<
                MigrationRoleStepObservationV1<OracleDimensionItemSetProposalV1>,
            >,
            Self::Error,
        >,
    > + Send {
        ready(tokio::task::block_in_place(|| {
            self.execute_oracle_item_discovery(checkpoint, context, access)
        }))
    }
}

impl
    AgentLoopStepExecutor<
        OracleDimensionItemSetReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OracleDimensionItemSetReviewV1>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleDimensionItemSetReviewerAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<
                MigrationRoleStepObservationV1<OracleDimensionItemSetReviewV1>,
            >,
            Self::Error,
        >,
    > + Send {
        ready(tokio::task::block_in_place(|| {
            self.execute_oracle_item_set_review(checkpoint, context, access)
        }))
    }
}

impl
    AgentLoopStepExecutor<
        OracleItemDeveloperAgentContextV1,
        MigrationRoleStepObservationV1<OracleItemDraftV1>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleItemDeveloperAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OracleItemDraftV1>>,
            Self::Error,
        >,
    > + Send {
        ready(tokio::task::block_in_place(|| {
            self.execute_oracle_item_development(checkpoint, context, access)
        }))
    }
}

impl
    AgentLoopStepExecutor<
        OracleItemReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OracleItemReviewV1>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OracleItemReviewerAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<OracleItemReviewV1>>,
            Self::Error,
        >,
    > + Send {
        ready(tokio::task::block_in_place(|| {
            self.execute_oracle_item_review(checkpoint, context, access)
        }))
    }
}

impl
    AgentLoopStepExecutor<
        OraclePortfolioCoherenceReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioCoherenceReviewV1>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &OraclePortfolioCoherenceReviewerAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<
                MigrationRoleStepObservationV1<OraclePortfolioCoherenceReviewV1>,
            >,
            Self::Error,
        >,
    > + Send {
        ready(tokio::task::block_in_place(|| {
            self.execute_oracle_portfolio_coherence_review(checkpoint, context, access)
        }))
    }
}

impl
    AgentLoopStepExecutor<
        CandidateExplorationAgentContextV1,
        MigrationRoleStepObservationV1<Option<CandidateProposalV1>>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &CandidateExplorationAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<Option<CandidateProposalV1>>>,
            Self::Error,
        >,
    > + Send {
        ready(self.execute_candidate_exploration(checkpoint, context, access))
    }
}
impl
    AgentLoopStepExecutor<
        CandidateRevisionAgentContextV1,
        MigrationRoleStepObservationV1<Option<CandidateProposalV1>>,
    > for MigrationAgentRuntimeExecutorV1
{
    type Error = MigrationAgentRuntimeError;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &CandidateRevisionAgentContextV1,
        access: &AgentStepAccessV1,
    ) -> impl Future<
        Output = Result<
            AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<Option<CandidateProposalV1>>>,
            Self::Error,
        >,
    > + Send {
        ready(self.execute_candidate_revision(checkpoint, context, access))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleDimensionItemsSubmissionV1 {
    schema_version: u16,
    dimension_id: ContentId<cairn_migration::OracleDimensionArtifact>,
    items: Vec<OracleItemStatement>,
}

struct OracleDimensionItemDiscoveryGateway {
    task_workspace: SirTaskWorkspace,
    limits: SirTaskLimits,
    dimension: OracleDimensionV1,
    claim: OracleClaimV1,
    previous: Option<OracleDimensionItemSetProposalV1>,
    review: Option<OracleDimensionItemSetReviewV1>,
    accepted: Option<OracleDimensionItemSetProposalV1>,
}

impl ToolGateway for OracleDimensionItemDiscoveryGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => {
                read_task_artifact(&self.task_workspace, self.limits, operation)
            }
            "migration-read-oracle-dimension" => {
                validate_operation(
                    operation,
                    "migration-read-oracle-dimension",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("Oracle dimension read requires current V1");
                }
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "dimension_id": self.dimension.identity().map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                    "dimension": self.dimension,
                    "claim_id": self.dimension.claim(),
                    "claim": self.claim,
                    "plane": self.dimension.plane(),
                    "concern": self.dimension.concern(),
                    "role": self.dimension.role(),
                    "previous_item_set": self.previous,
                    "review_feedback": self.review,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-submit-oracle-dimension-items" => {
                validate_operation(
                    operation,
                    "migration-submit-oracle-dimension-items",
                    ToolEffectClass::Pure,
                )?;
                let submission: OracleDimensionItemsSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let dimension_id = self
                    .dimension
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if submission.schema_version != SCHEMA_V1
                    || submission.dimension_id != dimension_id
                    || submission.items.is_empty()
                {
                    return rejected(
                        "Oracle item discovery changed or omitted its exact dimension",
                    );
                }
                let items = submission
                    .items
                    .into_iter()
                    .map(|statement| OracleItemV1::new(dimension_id, statement))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                let proposal = if let Some(previous) = &self.previous {
                    OracleDimensionItemSetProposalV1::revise(previous, items)
                } else {
                    OracleDimensionItemSetProposalV1::new(dimension_id, items)
                }
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &proposal)
                {
                    return rejected("Oracle dimension item set was already submitted differently");
                }
                let proposal_id = proposal
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(proposal);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "proposal_id": proposal_id,
                    "dimension_id": dimension_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Oracle item discovery role grant".to_owned(),
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleDimensionItemSetReviewSubmissionV1 {
    schema_version: u16,
    dimension_id: ContentId<cairn_migration::OracleDimensionArtifact>,
    proposal_id: ContentId<cairn_migration::OracleDimensionItemSetProposalArtifact>,
    decision: OracleDimensionItemSetReviewDecisionV1,
}

struct OracleDimensionItemSetReviewerGateway {
    task_workspace: SirTaskWorkspace,
    limits: SirTaskLimits,
    dimension: OracleDimensionV1,
    claim: OracleClaimV1,
    proposal: OracleDimensionItemSetProposalV1,
    accepted: Option<OracleDimensionItemSetReviewV1>,
}

impl ToolGateway for OracleDimensionItemSetReviewerGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => {
                read_task_artifact(&self.task_workspace, self.limits, operation)
            }
            "migration-read-oracle-dimension-items" => {
                validate_operation(
                    operation,
                    "migration-read-oracle-dimension-items",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("Oracle item-set read requires current V1");
                }
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "dimension_id": self.proposal.dimension(),
                    "dimension": self.dimension,
                    "claim": self.claim,
                    "proposal_id": self.proposal.identity().map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                    "proposal": self.proposal,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-submit-oracle-dimension-items-review" => {
                validate_operation(
                    operation,
                    "migration-submit-oracle-dimension-items-review",
                    ToolEffectClass::Pure,
                )?;
                let submission: OracleDimensionItemSetReviewSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let proposal_id = self
                    .proposal
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if submission.schema_version != SCHEMA_V1
                    || submission.dimension_id != self.proposal.dimension()
                    || submission.proposal_id != proposal_id
                {
                    return rejected("Oracle item-set Review changed its exact proposal");
                }
                let review = match submission.decision {
                    OracleDimensionItemSetReviewDecisionV1::Approved => {
                        OracleDimensionItemSetReviewV1::approved(&self.proposal)
                    }
                    OracleDimensionItemSetReviewDecisionV1::NeedsRevision { findings } => {
                        OracleDimensionItemSetReviewV1::needs_revision(&self.proposal, findings)
                    }
                }
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &review)
                {
                    return rejected("Oracle item-set Review was already submitted differently");
                }
                let review_id = review
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(review);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "dimension_id": self.proposal.dimension(),
                    "proposal_id": proposal_id,
                    "review_id": review_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Oracle item-set reviewer role grant".to_owned(),
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleItemDraftSubmissionV1 {
    schema_version: u16,
    item_id: ContentId<cairn_migration::OracleItemArtifact>,
    plans: Vec<OracleSubmittedCheckPlanV1>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReadOracleControlDiagnosticRequestV1 {
    schema_version: u16,
    receipt: ContentId<TrustedOracleControlReceiptArtifact>,
}

struct OracleControlDiagnosticMaterialV1 {
    receipt: ContentId<TrustedOracleControlReceiptArtifact>,
    failure_class: OracleControlFailureClassV1,
    summary: String,
    stdout: ContentId<ExecutionStdoutArtifact>,
    stdout_text: String,
    stderr: ContentId<ExecutionStderrArtifact>,
    stderr_text: String,
}

struct OracleItemDeveloperGateway {
    task_workspace: SirTaskWorkspace,
    limits: SirTaskLimits,
    oracle_workspace: OracleWorkspaceV1,
    item: OracleItemV1,
    dimension: OracleDimensionV1,
    claim: OracleClaimV1,
    previous: Option<OracleItemDraftV1>,
    review: Option<OracleItemReviewV1>,
    coherence: Option<OraclePortfolioCoherenceReviewV1>,
    admission: Option<OracleRevisionRequestV1>,
    control_diagnostics: Vec<OracleControlDiagnosticMaterialV1>,
    run: OracleStrategyRunV1,
    accepted: Option<OracleItemDraftV1>,
}

impl OracleItemDeveloperGateway {
    fn materialize_evidence(
        &self,
        evidence: Vec<OracleSubmittedCheckEvidenceV1>,
    ) -> Result<Vec<OracleCheckEvidenceV1>, ToolGatewayError> {
        let mut encoded = evidence
            .into_iter()
            .map(|value| {
                let typed = match value {
                    OracleSubmittedCheckEvidenceV1::SourceCitation { citation } => {
                        self.task_workspace
                            .validate_citation(&citation)
                            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                        OracleCheckEvidenceV1::SourceCitation { citation }
                    }
                    OracleSubmittedCheckEvidenceV1::AdmittedIntent { contract } => {
                        let contract = contract
                            .parse::<ContentId<cairn_migration::MigrationIntentContractArtifact>>()
                            .map_err(|_| {
                                ToolGatewayError::Rejected(
                                    "invalid admitted-intent identity".to_owned(),
                                )
                            })?;
                        if contract != self.oracle_workspace.admitted_intent() {
                            return Err(ToolGatewayError::Rejected(
                                "check evidence changed the admitted-intent identity".to_owned(),
                            ));
                        }
                        OracleCheckEvidenceV1::AdmittedIntent { contract }
                    }
                };
                cairn_codec::to_vec(&typed)
                    .map(|bytes| (bytes, typed))
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        encoded.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(encoded.into_iter().map(|(_, value)| value).collect())
    }

    fn read_control_diagnostic(
        &self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        validate_operation(
            operation,
            "migration-read-oracle-control-diagnostic",
            ToolEffectClass::ReadOnly,
        )?;
        let request: ReadOracleControlDiagnosticRequestV1 =
            decode_arguments(operation.argument_bytes())?;
        if request.schema_version != SCHEMA_V1 {
            return rejected("Oracle control diagnostic read requires current V1");
        }
        let diagnostic = offered_control_diagnostic(&self.control_diagnostics, request.receipt)?;
        CanonicalToolResult::from_value(&json!({
            "schema_version": SCHEMA_V1,
            "receipt": diagnostic.receipt,
            "failure_class": diagnostic.failure_class,
            "summary": diagnostic.summary,
            "stdout": {
                "artifact": diagnostic.stdout,
                "encoding": "utf-8-lossy",
                "text": diagnostic.stdout_text,
            },
            "stderr": {
                "artifact": diagnostic.stderr,
                "encoding": "utf-8-lossy",
                "text": diagnostic.stderr_text,
            },
        }))
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
    }
}

impl ToolGateway for OracleItemDeveloperGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => {
                read_task_artifact(&self.task_workspace, self.limits, operation)
            }
            "migration-read-oracle-item-conversation" => {
                validate_operation(
                    operation,
                    "migration-read-oracle-item-conversation",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("Oracle item conversation read requires current V1");
                }
                let item_id = self
                    .item
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "item_id": item_id,
                    "item": self.item,
                    "dimension": self.dimension,
                    "claim": self.claim,
                    "admitted_intent_id": self.oracle_workspace.admitted_intent(),
                    "previous_draft": self.previous,
                    "review_feedback": self.review,
                    "coherence_feedback": self.coherence.as_ref().map(|review| oracle_item_coherence_feedback(review, item_id)),
                    "admission_feedback": self.admission.as_ref().map(|request| oracle_item_admission_feedback(request, item_id)),
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-read-oracle-control-diagnostic" => self.read_control_diagnostic(operation),
            "migration-submit-oracle-item-draft" => {
                validate_operation(
                    operation,
                    "migration-submit-oracle-item-draft",
                    ToolEffectClass::Pure,
                )?;
                let submission: OracleItemDraftSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let item_id = self
                    .item
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if submission.schema_version != SCHEMA_V1
                    || submission.item_id != item_id
                    || submission.plans.is_empty()
                {
                    return rejected("Oracle item draft changed or omitted its exact item");
                }
                let plans = submission
                    .plans
                    .into_iter()
                    .map(|submitted| {
                        OracleCheckPlanV1::new(
                            item_id,
                            submitted.method,
                            submitted.objective,
                            submitted.setup,
                            submitted.observation,
                            submitted.pass_condition,
                            submitted.assertion,
                            self.materialize_evidence(submitted.evidence)?,
                        )
                        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let run = self
                    .run
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                let draft = match &self.previous {
                    Some(previous) => OracleItemDraftV1::revise(previous, run, plans),
                    None => OracleItemDraftV1::initial(self.item.clone(), run, plans),
                }
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &draft)
                {
                    return rejected("Oracle item draft was already submitted differently");
                }
                let draft_id = draft
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(draft);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "item_id": item_id,
                    "draft_id": draft_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Oracle item developer role grant".to_owned(),
            )),
        }
    }
}

fn offered_control_diagnostic(
    diagnostics: &[OracleControlDiagnosticMaterialV1],
    receipt: ContentId<TrustedOracleControlReceiptArtifact>,
) -> Result<&OracleControlDiagnosticMaterialV1, ToolGatewayError> {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.receipt == receipt)
        .ok_or_else(|| {
            ToolGatewayError::Rejected(
                "control receipt is not offered by this exact item revision".to_owned(),
            )
        })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleItemReviewSubmissionV1 {
    schema_version: u16,
    item_id: ContentId<cairn_migration::OracleItemArtifact>,
    draft_id: ContentId<cairn_migration::OracleItemDraftArtifact>,
    decision: OracleItemReviewDecisionV1,
}

struct OracleItemReviewerGateway {
    task_workspace: SirTaskWorkspace,
    limits: SirTaskLimits,
    dimension: OracleDimensionV1,
    claim: OracleClaimV1,
    draft: OracleItemDraftV1,
    accepted: Option<OracleItemReviewV1>,
}

impl ToolGateway for OracleItemReviewerGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => {
                read_task_artifact(&self.task_workspace, self.limits, operation)
            }
            "migration-read-oracle-item-draft" => {
                validate_operation(
                    operation,
                    "migration-read-oracle-item-draft",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("Oracle item draft read requires current V1");
                }
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "dimension": self.dimension,
                    "claim": self.claim,
                    "draft_id": self.draft.identity().map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                    "draft": self.draft,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-submit-oracle-item-review" => {
                validate_operation(
                    operation,
                    "migration-submit-oracle-item-review",
                    ToolEffectClass::Pure,
                )?;
                let submission: OracleItemReviewSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let item_id = self
                    .draft
                    .item()
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                let draft_id = self
                    .draft
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if submission.schema_version != SCHEMA_V1
                    || submission.item_id != item_id
                    || submission.draft_id != draft_id
                {
                    return rejected("Oracle item review changed its exact item or draft");
                }
                let review = match submission.decision {
                    OracleItemReviewDecisionV1::Approved => {
                        OracleItemReviewV1::approved(&self.draft)
                    }
                    OracleItemReviewDecisionV1::NeedsRevision { findings } => {
                        OracleItemReviewV1::needs_revision(&self.draft, findings)
                    }
                }
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &review)
                {
                    return rejected("Oracle item review was already submitted differently");
                }
                let review_id = review
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(review);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "item_id": item_id,
                    "draft_id": draft_id,
                    "review_id": review_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Oracle item reviewer role grant".to_owned(),
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OraclePortfolioCoherenceReviewSubmissionV1 {
    schema_version: u16,
    portfolio_id: ContentId<cairn_migration::OraclePortfolioProposalArtifact>,
    decision: OraclePortfolioCoherenceDecisionV1,
}

struct OraclePortfolioCoherenceReviewerGateway {
    claims: Vec<OracleClaimV1>,
    dimensions: Vec<OracleDimensionV1>,
    portfolio: OraclePortfolioProposalV1,
    accepted: Option<OraclePortfolioCoherenceReviewV1>,
}

impl ToolGateway for OraclePortfolioCoherenceReviewerGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-oracle-portfolio" => {
                validate_operation(
                    operation,
                    "migration-read-oracle-portfolio",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("Oracle portfolio read requires current V1");
                }
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "claims": self.claims,
                    "dimensions": self.dimensions,
                    "portfolio_id": self.portfolio.identity().map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                    "portfolio": self.portfolio,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-submit-oracle-portfolio-coherence-review" => {
                validate_operation(
                    operation,
                    "migration-submit-oracle-portfolio-coherence-review",
                    ToolEffectClass::Pure,
                )?;
                let submission: OraclePortfolioCoherenceReviewSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let portfolio_id = self
                    .portfolio
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if submission.schema_version != SCHEMA_V1 || submission.portfolio_id != portfolio_id
                {
                    return rejected("Oracle coherence Review changed its exact portfolio");
                }
                let review = match submission.decision {
                    OraclePortfolioCoherenceDecisionV1::Approved => {
                        OraclePortfolioCoherenceReviewV1::approved(&self.portfolio)
                    }
                    OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings } => {
                        OraclePortfolioCoherenceReviewV1::needs_revision(&self.portfolio, findings)
                    }
                }
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &review)
                {
                    return rejected("Oracle coherence Review was already submitted differently");
                }
                let review_id = review
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(review);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "portfolio_id": portfolio_id,
                    "review_id": review_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Oracle portfolio coherence reviewer role grant"
                    .to_owned(),
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleSubmittedCheckPlanV1 {
    method: OracleCheckMethodV1,
    objective: OracleCheckObjective,
    setup: OracleCheckSetup,
    observation: OracleCheckObservation,
    pass_condition: OracleCheckPassCondition,
    assertion: OracleCheckAssertionV1,
    evidence: Vec<OracleSubmittedCheckEvidenceV1>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleWholePortfolioItemSubmissionV1 {
    statement: OracleItemStatement,
    plans: Vec<OracleSubmittedCheckPlanV1>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleWholePortfolioDimensionSubmissionV1 {
    dimension_id: ContentId<cairn_migration::OracleDimensionArtifact>,
    items: Vec<OracleWholePortfolioItemSubmissionV1>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleWholePortfolioSubmissionV1 {
    schema_version: u16,
    authority_id: ContentId<cairn_migration::OracleWholePortfolioProposalAuthorityArtifact>,
    dimensions: Vec<OracleWholePortfolioDimensionSubmissionV1>,
}

struct OracleWholePortfolioGateway {
    task_workspace: SirTaskWorkspace,
    limits: SirTaskLimits,
    recovery_input: IntentRecoveryInputV1,
    workspace: OracleWorkspaceV1,
    dimensions: Vec<OracleDimensionV1>,
    claims: Vec<OracleClaimV1>,
    catalog: OracleStrategyCatalogV1,
    authority: OracleWholePortfolioProposalAuthorityV1,
    previous: Option<OraclePortfolioProposalV1>,
    admission: Option<OracleRevisionRequestV1>,
    runs: Vec<OracleStrategyRunV1>,
    accepted: Option<OraclePortfolioProposalV1>,
}

impl OracleWholePortfolioGateway {
    fn materialize_evidence(
        &self,
        evidence: Vec<OracleSubmittedCheckEvidenceV1>,
    ) -> Result<Vec<OracleCheckEvidenceV1>, ToolGatewayError> {
        let mut encoded = evidence
            .into_iter()
            .map(|value| {
                let typed = match value {
                    OracleSubmittedCheckEvidenceV1::SourceCitation { citation } => {
                        self.task_workspace
                            .validate_citation(&citation)
                            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                        OracleCheckEvidenceV1::SourceCitation { citation }
                    }
                    OracleSubmittedCheckEvidenceV1::AdmittedIntent { contract } => {
                        let contract = contract
                            .parse::<ContentId<cairn_migration::MigrationIntentContractArtifact>>()
                            .map_err(|_| {
                                ToolGatewayError::Rejected(
                                    "invalid admitted-intent identity".to_owned(),
                                )
                            })?;
                        if contract != self.workspace.admitted_intent() {
                            return Err(ToolGatewayError::Rejected(
                                "check evidence changed the admitted-intent identity".to_owned(),
                            ));
                        }
                        OracleCheckEvidenceV1::AdmittedIntent { contract }
                    }
                };
                cairn_codec::to_vec(&typed)
                    .map(|bytes| (bytes, typed))
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        encoded.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(encoded.into_iter().map(|(_, value)| value).collect())
    }
}

fn whole_portfolio_dimension_scope(
    dimensions: &[OracleDimensionV1],
) -> Result<Vec<Value>, ToolGatewayError> {
    dimensions
        .iter()
        .map(|dimension| {
            Ok(json!({
                "dimension_id": dimension.identity().map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                "dimension": dimension,
            }))
        })
        .collect()
}

fn whole_portfolio_claim_scope(claims: &[OracleClaimV1]) -> Result<Vec<Value>, ToolGatewayError> {
    claims
        .iter()
        .map(|claim| {
            Ok(json!({
                "claim_id": claim.identity().map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                "claim": claim,
            }))
        })
        .collect()
}

impl ToolGateway for OracleWholePortfolioGateway {
    #[allow(
        clippy::too_many_lines,
        reason = "one strict submission boundary validates the complete dimension/item/plan tree"
    )]
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => {
                read_task_artifact(&self.task_workspace, self.limits, operation)
            }
            "migration-read-oracle-whole-portfolio-scope" => {
                validate_operation(
                    operation,
                    "migration-read-oracle-whole-portfolio-scope",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("whole-portfolio scope read requires current V1");
                }
                let dimensions = whole_portfolio_dimension_scope(&self.dimensions)?;
                let claims = whole_portfolio_claim_scope(&self.claims)?;
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "authority_id": self.authority.identity().map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                    "workspace": self.workspace,
                    "task_context": self.recovery_input,
                    "dimensions": dimensions,
                    "claims": claims,
                    "previous_portfolio": self.previous,
                    "admission_feedback": self.admission,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-submit-oracle-whole-portfolio" => {
                validate_operation(
                    operation,
                    "migration-submit-oracle-whole-portfolio",
                    ToolEffectClass::Pure,
                )?;
                let mut submission: OracleWholePortfolioSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let authority_id = self
                    .authority
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if submission.schema_version != SCHEMA_V1
                    || submission.authority_id != authority_id
                    || submission.dimensions.is_empty()
                {
                    return rejected("whole-portfolio submission changed its exact authority");
                }
                submission
                    .dimensions
                    .sort_by_key(|value| value.dimension_id.to_wire());
                let mut expected = self
                    .dimensions
                    .iter()
                    .map(OracleDimensionV1::identity)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                expected.sort_by_key(ContentId::to_wire);
                let offered = submission
                    .dimensions
                    .iter()
                    .map(|value| value.dimension_id)
                    .collect::<Vec<_>>();
                if offered != expected {
                    return rejected(
                        "whole-portfolio submission must cover every exact dimension once",
                    );
                }
                let mut accepted_items = Vec::new();
                for submitted_dimension in submission.dimensions {
                    if submitted_dimension.items.is_empty() {
                        return rejected("whole-portfolio dimension omitted all items");
                    }
                    let dimension = self
                        .dimensions
                        .iter()
                        .find(|dimension| {
                            dimension
                                .identity()
                                .is_ok_and(|identity| identity == submitted_dimension.dimension_id)
                        })
                        .ok_or_else(|| {
                            ToolGatewayError::Rejected(
                                "whole-portfolio dimension is not offered".to_owned(),
                            )
                        })?;
                    let run = OracleStrategyRunV1::new(
                        self.workspace
                            .identity()
                            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                        dimension,
                        OracleStrategyName::new(MODEL_BACKED_SYNTHESIS_STRATEGY)
                            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                        &self.catalog,
                    )
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                    let run_id = run
                        .identity()
                        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                    self.runs.push(run);
                    for submitted_item in submitted_dimension.items {
                        if submitted_item.plans.is_empty() {
                            return rejected("whole-portfolio item omitted all plans");
                        }
                        let item = OracleItemV1::new(
                            submitted_dimension.dimension_id,
                            submitted_item.statement,
                        )
                        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                        let item_id = item
                            .identity()
                            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                        let plans = submitted_item
                            .plans
                            .into_iter()
                            .map(|submitted| {
                                OracleCheckPlanV1::new(
                                    item_id,
                                    submitted.method,
                                    submitted.objective,
                                    submitted.setup,
                                    submitted.observation,
                                    submitted.pass_condition,
                                    submitted.assertion,
                                    self.materialize_evidence(submitted.evidence)?,
                                )
                                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let draft = OracleItemDraftV1::initial(item, run_id, plans)
                            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                        accepted_items.push(
                            cairn_migration::OracleAcceptedItemV1::from_whole_portfolio_episode(
                                &draft,
                                &self.authority,
                            )
                            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
                        );
                    }
                }
                let portfolio = OraclePortfolioProposalV1::assemble(
                    &self.workspace,
                    self.dimensions.clone(),
                    accepted_items,
                )
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &portfolio)
                {
                    return rejected("whole-portfolio proposal was already submitted differently");
                }
                let portfolio_id = portfolio
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(portfolio);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "authority_id": authority_id,
                    "portfolio_id": portfolio_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Oracle whole-portfolio role grant".to_owned(),
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
enum OracleSubmittedCheckEvidenceV1 {
    SourceCitation {
        citation: cairn_migration::SirSourceCitationV1,
    },
    AdmittedIntent {
        contract: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CurrentSchemaRequestV1 {
    schema_version: u16,
}

/// Builds the complete task-generic migration tool registry selected by role hooks.
///
/// # Errors
///
/// Returns an error if a built-in registration, descriptor, or semantic identity is invalid.
pub fn migration_tool_registry() -> Result<ToolRegistry, MigrationAgentRuntimeError> {
    let mut registry = ToolRegistry::default();
    for tool in [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::RunEvidenceExperiment,
        MigrationAgentToolV1::SubmitSir,
        MigrationAgentToolV1::ReadOracleWholePortfolioScope,
        MigrationAgentToolV1::SubmitOracleWholePortfolio,
        MigrationAgentToolV1::ReadOracleDimension,
        MigrationAgentToolV1::SubmitOracleDimensionItems,
        MigrationAgentToolV1::ReadOracleDimensionItems,
        MigrationAgentToolV1::SubmitOracleDimensionItemsReview,
        MigrationAgentToolV1::ReadOracleItemConversation,
        MigrationAgentToolV1::ReadOracleControlDiagnostic,
        MigrationAgentToolV1::SubmitOracleItemDraft,
        MigrationAgentToolV1::ReadOracleItemDraft,
        MigrationAgentToolV1::SubmitOracleItemReview,
        MigrationAgentToolV1::ReadOraclePortfolio,
        MigrationAgentToolV1::SubmitOraclePortfolioCoherenceReview,
        MigrationAgentToolV1::ReadAdmittedOracle,
        MigrationAgentToolV1::SubmitCandidate,
        MigrationAgentToolV1::ReadCandidateObservation,
        MigrationAgentToolV1::SubmitCandidateRevision,
    ] {
        let name = tool
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let effect = match tool {
            MigrationAgentToolV1::RunEvidenceExperiment => ToolEffectClass::Idempotent,
            MigrationAgentToolV1::ReadTaskArtifact
            | MigrationAgentToolV1::ReadOracleWholePortfolioScope
            | MigrationAgentToolV1::ReadOracleDimension
            | MigrationAgentToolV1::ReadOracleDimensionItems
            | MigrationAgentToolV1::ReadOracleItemConversation
            | MigrationAgentToolV1::ReadOracleControlDiagnostic
            | MigrationAgentToolV1::ReadOracleItemDraft
            | MigrationAgentToolV1::ReadOraclePortfolio
            | MigrationAgentToolV1::ReadAdmittedOracle
            | MigrationAgentToolV1::ReadCandidateObservation => ToolEffectClass::ReadOnly,
            _ => ToolEffectClass::Pure,
        };
        let descriptor = ContentId::<ToolDescriptorArtifact>::derive(
            &cairn_codec::to_vec(&json!({
                "schema_version": SCHEMA_V1,
                "name": name,
                "effect": effect,
            }))
            .map_err(MigrationAgentRuntimeError::domain)?,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        registry
            .register(
                ToolRegistration::new(
                    name,
                    ToolImplementationVersion::new(TOOL_VERSION)
                        .map_err(MigrationAgentRuntimeError::domain)?,
                    effect,
                ),
                descriptor,
            )
            .map_err(MigrationAgentRuntimeError::domain)?;
    }
    Ok(registry)
}

struct SirProjectionV1 {
    instruction: ContentId<InstructionBlock>,
    tool_catalog: ContentId<ToolCatalog>,
    history: ContentId<HistoryItem>,
    context: ContentId<ContextBlock>,
    policy: ContentId<PolicyDocument>,
    user_text: String,
}

fn archive_role_projection(
    content: &mut SqliteContentStore,
    instruction_text: &str,
    tools: &[NativeToolDefinition],
    model_context: &Value,
    user_text: String,
) -> Result<SirProjectionV1, MigrationAgentRuntimeError> {
    let user_text = model_visible_role_user_text(user_text, model_context)?;
    Ok(SirProjectionV1 {
        instruction: put_json::<InstructionBlock>(content, &json!({"text": instruction_text}))?,
        tool_catalog: put_json::<ToolCatalog>(
            content,
            &json!({
                "schema_version": SCHEMA_V1,
                "tools": tools.iter().map(|tool| json!({
                    "name": tool.name.as_str(),
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                    "strict": tool.strict,
                })).collect::<Vec<_>>(),
            }),
        )?,
        history: put_json::<HistoryItem>(content, &json!({"role":"user", "content":user_text}))?,
        context: put_json::<ContextBlock>(content, model_context)?,
        policy: put_json::<PolicyDocument>(
            content,
            &json!({
                "schema_version": SCHEMA_V1,
                "filesystem":"frozen-task-bundle-only",
                "network":"none",
                "knowledge":"none",
                "proposal_authority":"none",
                "review_authority":"item-reviewer-only",
                "admission_authority":"none",
                "hidden_material":"unavailable",
            }),
        )?,
        user_text,
    })
}

fn model_visible_role_user_text(
    mut request: String,
    model_context: &Value,
) -> Result<String, MigrationAgentRuntimeError> {
    let context = String::from_utf8(
        cairn_codec::to_vec(model_context).map_err(MigrationAgentRuntimeError::domain)?,
    )
    .map_err(MigrationAgentRuntimeError::domain)?;
    request.push_str("\n\nFrozen role context:\n");
    request.push_str(&context);
    Ok(request)
}

fn archive_sir_projection(
    content: &mut SqliteContentStore,
    task: &RuntimeTaskV1,
    tools: &[NativeToolDefinition],
) -> Result<SirProjectionV1, MigrationAgentRuntimeError> {
    let model_context = json!({
        "schema_version": SCHEMA_V1,
        "knowledge_snapshot": {"kind":"empty"},
        "recovery_input_id": task.recovery_input.identity().map_err(MigrationAgentRuntimeError::domain)?,
        "recovery_input": task.recovery_input,
        "task_artifacts": task.workspace.bundle().artifacts(),
    });
    let context_text = String::from_utf8(
        cairn_codec::to_vec(&model_context).map_err(MigrationAgentRuntimeError::domain)?,
    )
    .map_err(MigrationAgentRuntimeError::domain)?;
    let user_text = format!(
        "Recover higher-order intent from the frozen caller declaration and offered task evidence, then submit one complete typed proposal.\n\nFrozen recovery input:\n{context_text}"
    );
    Ok(SirProjectionV1 {
        instruction: put_json::<InstructionBlock>(content, &json!({"text": SIR_INSTRUCTION}))?,
        tool_catalog: put_json::<ToolCatalog>(
            content,
            &json!({
                "schema_version": SCHEMA_V1,
                "tools": tools.iter().map(|tool| json!({
                    "name": tool.name.as_str(),
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                    "strict": tool.strict,
                })).collect::<Vec<_>>(),
            }),
        )?,
        history: put_json::<HistoryItem>(content, &json!({"role":"user", "content":user_text}))?,
        context: put_json::<ContextBlock>(content, &model_context)?,
        policy: put_json::<PolicyDocument>(
            content,
            &json!({
                "schema_version": SCHEMA_V1,
                "filesystem":"frozen-task-bundle-only",
                "network":"none",
                "proposal_authority":"none",
                "hidden_material":"unavailable",
            }),
        )?,
        user_text,
    })
}

fn oracle_item_coherence_feedback(
    review: &OraclePortfolioCoherenceReviewV1,
    item: ContentId<cairn_migration::OracleItemArtifact>,
) -> Value {
    let findings = match review.decision() {
        OraclePortfolioCoherenceDecisionV1::Approved => Vec::new(),
        OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings } => findings
            .iter()
            .filter(|finding| finding.affected_items().items().contains(&item))
            .map(|finding| {
                json!({
                    "affected_items": finding.affected_items().items(),
                    "issue": finding.issue(),
                    "explanation": finding.explanation().as_str(),
                    "required_change": finding.required_change().as_str(),
                })
            })
            .collect(),
    };
    json!({
        "portfolio_id": review.portfolio(),
        "item_id": item,
        "findings": findings,
    })
}

#[derive(Clone, Copy)]
struct OracleControlDiagnosticReadByteLimit(usize);

impl OracleControlDiagnosticReadByteLimit {
    const fn get(self) -> usize {
        self.0
    }
}

const ORACLE_CONTROL_DIAGNOSTIC_READ_LIMIT: OracleControlDiagnosticReadByteLimit =
    OracleControlDiagnosticReadByteLimit(16 * 1024);

struct BoundedDiagnosticBytes {
    bytes: Vec<u8>,
    limit: OracleControlDiagnosticReadByteLimit,
}

impl Write for BoundedDiagnosticBytes {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(buffer.len()) > self.limit.get() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Oracle control diagnostic exceeds its bounded read limit",
            ));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn read_control_diagnostic_artifact<T: ContentType>(
    content: &SqliteContentStore,
    identity: ContentId<T>,
) -> Result<String, MigrationAgentRuntimeError> {
    let mut output = BoundedDiagnosticBytes {
        bytes: Vec::new(),
        limit: ORACLE_CONTROL_DIAGNOSTIC_READ_LIMIT,
    };
    content
        .write_to(&identity, &mut output)
        .map_err(MigrationAgentRuntimeError::domain)?;
    Ok(String::from_utf8_lossy(&output.bytes).into_owned())
}

fn load_oracle_control_diagnostics(
    content: &SqliteContentStore,
    request: &OracleRevisionRequestV1,
    item: ContentId<cairn_migration::OracleItemArtifact>,
) -> Result<Vec<OracleControlDiagnosticMaterialV1>, MigrationAgentRuntimeError> {
    request
        .evidence()
        .receipts()
        .iter()
        .filter(|receipt| receipt.item() == item)
        .filter(|receipt| {
            receipt
                .failure_class()
                .is_some_and(OracleControlFailureClassV1::requires_oracle_revision)
        })
        .map(|receipt| {
            let diagnostic = receipt
                .diagnostic()
                .ok_or(MigrationAgentRuntimeError::TaskBinding)?;
            Ok(OracleControlDiagnosticMaterialV1 {
                receipt: receipt.receipt(),
                failure_class: diagnostic.failure_class(),
                summary: diagnostic.summary().as_str().to_owned(),
                stdout: diagnostic.stdout(),
                stdout_text: read_control_diagnostic_artifact(content, diagnostic.stdout())?,
                stderr: diagnostic.stderr(),
                stderr_text: read_control_diagnostic_artifact(content, diagnostic.stderr())?,
            })
        })
        .collect()
}

fn oracle_item_admission_feedback(
    request: &OracleRevisionRequestV1,
    item: ContentId<cairn_migration::OracleItemArtifact>,
) -> Value {
    let attempt = request.attempt();
    let outcome = request.outcome();
    let evidence = request.evidence();
    let disposition = outcome.claims().iter().find_map(|claim| {
        if claim.rejected_items().contains(&item) {
            Some("rejected")
        } else if claim.unresolved_items().contains(&item) {
            Some("unresolved")
        } else {
            None
        }
    });
    let controls = attempt
        .required_controls()
        .iter()
        .filter(|obligation| obligation.item() == item)
        .filter_map(|obligation| {
            let receipt = evidence.receipts().iter().find(|receipt| {
                receipt.item() == item
                    && receipt.control() == obligation.control()
                    && receipt.mechanism() == obligation.mechanism()
            });
            let receipt = receipt.filter(|receipt| receipt.result() == OracleControlResultV1::Failed);
            let receipt = receipt?;
            let diagnostic = receipt.diagnostic();
            Some(json!({
                "control": obligation.control(),
                "mechanism": obligation.mechanism(),
                "result": "failed",
                "receipt": receipt.receipt(),
                "diagnostic": diagnostic.map(|diagnostic| json!({
                    "failure_class": diagnostic.failure_class(),
                    "summary": diagnostic.summary().as_str(),
                    "stdout": diagnostic.stdout(),
                    "stderr": diagnostic.stderr(),
                })),
                "required_change": "Read this exact receipt with migration-read-oracle-control-diagnostic, then revise this item to address the artifact-owned failure. Do not treat the failed receipt as authority.",
            }))
        })
        .collect::<Vec<_>>();
    json!({
        "gate":"admission",
        "item_id":item,
        "disposition":disposition,
        "failed_controls":controls,
    })
}

fn oracle_admission_targets_item(
    request: &OracleRevisionRequestV1,
    item: ContentId<cairn_migration::OracleItemArtifact>,
) -> bool {
    let attempt = request.attempt();
    let outcome = request.outcome();
    let evidence = request.evidence();
    outcome.claims().iter().any(|claim| {
        claim.unresolved_items().contains(&item) || claim.rejected_items().contains(&item)
    }) || attempt.required_controls().iter().any(|obligation| {
        obligation.item() == item
            && !evidence.receipts().iter().any(|receipt| {
                receipt.item() == item
                    && receipt.control() == obligation.control()
                    && receipt.mechanism() == obligation.mechanism()
                    && receipt.result() == OracleControlResultV1::Passed
            })
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReadTaskArtifactRequestV1 {
    schema_version: u16,
    path: SirTaskArtifactPath,
    start_line: SirSourceLineNumber,
    line_count: SirReadLineLimit,
}

struct SirGateway {
    workspace: SirTaskWorkspace,
    recovery_input: IntentRecoveryInputV1,
    limits: SirTaskLimits,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    accepted: Option<IntentHypothesisSetProposalV1>,
}

impl ToolGateway for SirGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => self.read(operation),
            "migration-submit-sir" => self.submit(operation),
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the SIR role grant".to_owned(),
            )),
        }
    }
}

impl SirGateway {
    fn read(
        &self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        read_task_artifact(&self.workspace, self.limits, operation)
    }

    fn submit(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        validate_operation(operation, "migration-submit-sir", ToolEffectClass::Pure)?;
        let submission: SirProposalSubmissionV1 = decode_arguments(operation.argument_bytes())?;
        submission
            .validate_against(&self.workspace, &self.recovery_input)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let proposal = IntentHypothesisSetProposalV1::new(
            self.recovery_input
                .identity()
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
            self.episode_id,
            self.model_configuration,
            submission,
        );
        let identity = proposal
            .identity()
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        if let Some(accepted) = &self.accepted {
            if accepted != &proposal {
                return rejected("a different SIR proposal was already accepted");
            }
        } else {
            self.accepted = Some(proposal);
        }
        CanonicalToolResult::from_value(&json!({
            "schema_version": SCHEMA_V1,
            "accepted_proposal": identity,
        }))
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
    }
}

fn read_task_artifact(
    workspace: &SirTaskWorkspace,
    limits: SirTaskLimits,
    operation: &PreparedToolOperation,
) -> Result<CanonicalToolResult, ToolGatewayError> {
    validate_operation(
        operation,
        "migration-read-task-artifact",
        ToolEffectClass::ReadOnly,
    )?;
    let request: ReadTaskArtifactRequestV1 = decode_arguments(operation.argument_bytes())?;
    if request.schema_version != SCHEMA_V1 {
        return rejected("task read schema_version must be the current V1 value 1");
    }
    if request.line_count.get() > limits.max_read_lines.get() {
        return rejected(&format!(
            "task read line_count {} exceeds this task's maximum {}; retry with line_count at most {} and continue from the next start_line",
            request.line_count.get(),
            limits.max_read_lines.get(),
            limits.max_read_lines.get()
        ));
    }
    let artifact = workspace
        .artifact(&request.path)
        .ok_or_else(|| ToolGatewayError::Rejected("task artifact is not offered".to_owned()))?;
    if request.start_line.get() > artifact.line_count().get() {
        return rejected("task read starts outside the offered artifact");
    }
    let source = workspace.source(&request.path).ok_or_else(|| {
        ToolGatewayError::Rejected("task artifact bytes are unavailable".to_owned())
    })?;
    let start = usize::try_from(request.start_line.get() - 1)
        .map_err(|_| ToolGatewayError::Rejected("source line overflow".to_owned()))?;
    let requested = usize::try_from(request.line_count.get())
        .map_err(|_| ToolGatewayError::Rejected("source line overflow".to_owned()))?;
    let lines = source
        .lines()
        .skip(start)
        .take(requested)
        .collect::<Vec<_>>();
    let returned_bytes = lines.iter().try_fold(0_u64, |total, line| {
        total
            .checked_add(u64::try_from(line.len()).map_err(|_| {
                ToolGatewayError::Rejected("source byte length overflow".to_owned())
            })?)
            .ok_or_else(|| ToolGatewayError::Rejected("source byte length overflow".to_owned()))
    })?;
    if returned_bytes > limits.max_read_bytes.get() {
        return rejected(&format!(
            "task read would return {returned_bytes} source bytes, exceeding this task's maximum {}; retry the same start_line with a smaller line_count",
            limits.max_read_bytes.get()
        ));
    }
    CanonicalToolResult::from_value(&json!({
            "schema_version": SCHEMA_V1,
            "path": request.path,
            "artifact_identity": artifact.identity(),
            "lines": lines.iter().enumerate().map(|(offset, text)| json!({
                "line": request.start_line.get().saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)),
                "text": text,
            })).collect::<Vec<_>>(),
        }))
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
}

#[allow(
    clippy::too_many_lines,
    reason = "the exact provider schema stays visibly aligned with the current typed SIR submission"
)]
/// Frozen Candidate authority a proposal episode may read, plus the submission it produced.
///
/// The contract is the only statement of required behaviour the episode is authorized to rely on.
/// The CUDA source reaches it as a task artifact, which is evidence about intent and never a
/// specification, so the two arrive through different tools and cannot be confused for each other.
struct CandidateExplorationGateway {
    task_workspace: SirTaskWorkspace,
    limits: SirTaskLimits,
    contract: CandidateOracleContractV1,
    contract_id: ContentId<cairn_migration::CandidateOracleContractArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    accepted: Option<CandidateProposalV1>,
}

impl ToolGateway for CandidateExplorationGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => {
                read_task_artifact(&self.task_workspace, self.limits, operation)
            }
            "migration-read-admitted-oracle" => {
                validate_operation(
                    operation,
                    "migration-read-admitted-oracle",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("admitted Oracle read requires current V1");
                }
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "oracle_contract_id": self.contract_id,
                    "admitted_claims": self.contract.admitted_claims(),
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-submit-candidate" => {
                validate_operation(
                    operation,
                    "migration-submit-candidate",
                    ToolEffectClass::Pure,
                )?;
                let submission: CandidateProposalSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let proposal = CandidateProposalV1::new(
                    self.contract_id,
                    self.episode_id,
                    self.model_configuration,
                    submission,
                )
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &proposal)
                {
                    return rejected("candidate proposal was already submitted differently");
                }
                let proposal_id = proposal
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(proposal);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "proposal_id": proposal_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Candidate exploration role grant".to_owned(),
            )),
        }
    }
}

/// The parent proposal a build refuted, and the bounded diagnostic that refuted it.
///
/// The compiler's own words are handed back verbatim. An exit code or a receipt identity says a
/// build failed; only the diagnostic says what to change, and guessing from the former is how the
/// last attributed failure invented an API that did not exist.
struct CandidateBuildDiagnosticV1 {
    receipt: ContentId<cairn_execution::ExecutionReceiptArtifact>,
    outcome: cairn_execution::ExecutionOutcome,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

struct CandidateRevisionGateway {
    task_workspace: SirTaskWorkspace,
    limits: SirTaskLimits,
    contract: CandidateOracleContractV1,
    contract_id: ContentId<cairn_migration::CandidateOracleContractArtifact>,
    parent: CandidateProposalV1,
    parent_id: ContentId<cairn_migration::CandidateProposalArtifact>,
    diagnostic: CandidateBuildDiagnosticV1,
    notice: Option<cairn_migration::CandidateSearchNoticeV1>,
    remaining: cairn_migration::CandidateIterationsRemaining,
    episode_id: cairn_protocol::EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    accepted: Option<CandidateProposalV1>,
}

impl ToolGateway for CandidateRevisionGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            "migration-read-task-artifact" => {
                read_task_artifact(&self.task_workspace, self.limits, operation)
            }
            "migration-read-admitted-oracle" => {
                validate_operation(
                    operation,
                    "migration-read-admitted-oracle",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("admitted Oracle read requires current V1");
                }
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "oracle_contract_id": self.contract_id,
                    "admitted_claims": self.contract.admitted_claims(),
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-read-candidate-observation" => {
                validate_operation(
                    operation,
                    "migration-read-candidate-observation",
                    ToolEffectClass::ReadOnly,
                )?;
                let request: CurrentSchemaRequestV1 = decode_arguments(operation.argument_bytes())?;
                if request.schema_version != SCHEMA_V1 {
                    return rejected("candidate observation read requires current V1");
                }
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "parent_proposal_id": self.parent_id,
                    "parent_files": self.parent.submission().files(),
                    "parent_primary_source": self.parent.submission().primary_source(),
                    "build_receipt_id": self.diagnostic.receipt,
                    "build_outcome": format!("{:?}", self.diagnostic.outcome),
                    "build_exit_code": self.diagnostic.exit_code,
                    "build_stdout": self.diagnostic.stdout,
                    "build_stderr": self.diagnostic.stderr,
                    "build_attempts_remaining": self.remaining,
                    "controller_notice": self.notice,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            "migration-submit-candidate-revision" => {
                validate_operation(
                    operation,
                    "migration-submit-candidate-revision",
                    ToolEffectClass::Pure,
                )?;
                let submission: CandidateProposalSubmissionV1 =
                    decode_arguments(operation.argument_bytes())?;
                let proposal = CandidateProposalV1::new(
                    self.contract_id,
                    self.episode_id,
                    self.model_configuration,
                    submission,
                )
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted != &proposal)
                {
                    return rejected("candidate revision was already submitted differently");
                }
                let proposal_id = proposal
                    .identity()
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.accepted = Some(proposal);
                CanonicalToolResult::from_value(&json!({
                    "schema_version": SCHEMA_V1,
                    "proposal_id": proposal_id,
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is outside the Candidate revision role grant".to_owned(),
            )),
        }
    }
}

fn candidate_revision_native_tools(
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let mut tools = candidate_exploration_native_tools(limits)?;
    tools.truncate(2);
    tools.push(NativeToolDefinition {
        name: MigrationAgentToolV1::ReadCandidateObservation
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description:
            "Read the exact previous proposal and the build that refused it, including the compiler's own bounded stdout and stderr, and how many build attempts remain."
                .to_owned(),
        input_schema: json!({
            "type":"object",
            "properties":{"schema_version":{"type":"integer","const":1}},
            "required":["schema_version"],
            "additionalProperties":false
        }),
        strict: true,
    });
    tools.push(NativeToolDefinition {
        name: MigrationAgentToolV1::SubmitCandidateRevision
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description:
            "Submit the complete revised implementation: every source file the build needs, not only the ones you changed."
                .to_owned(),
        input_schema: candidate_submission_schema(),
        strict: true,
    });
    Ok(tools)
}

fn candidate_exploration_native_tools(
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let mut tools = vec![read_task_artifact_native_tool(limits)?];
    tools.push(NativeToolDefinition {
        name: MigrationAgentToolV1::ReadAdmittedOracle
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description:
            "Read the admitted Oracle contract for this task: the admitted intent claims and the obligations that define them. This is what a candidate has to satisfy, and it is the only statement of required behaviour you may rely on."
                .to_owned(),
        input_schema: json!({
            "type":"object",
            "properties":{"schema_version":{"type":"integer","const":1}},
            "required":["schema_version"],
            "additionalProperties":false
        }),
        strict: true,
    });
    tools.push(NativeToolDefinition {
        name: MigrationAgentToolV1::SubmitCandidate
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description:
            "Submit one complete implementation: every source file the build needs, which file holds the kernel entry point, and the assumptions you could not verify."
                .to_owned(),
        input_schema: candidate_submission_schema(),
        strict: true,
    });
    Ok(tools)
}

/// The one submission shape both proposal roles offer.
///
/// A revision replaces the whole source tree rather than patching it, so the two roles describe
/// the same object; letting them drift would mean the model is shown one contract and judged by
/// another.
fn candidate_submission_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "schema_version":{"type":"integer","const":1},
            "files":{
                "type":"array","minItems":1,"maxItems":32,
                "items":{
                    "type":"object",
                    "properties":{
                        "path":{"type":"string","minLength":1,"maxLength":512,"description":"Relative path inside the candidate source tree. No absolute path and no parent-directory segment."},
                        "source":{"type":"string","minLength":1,"maxLength":262_144}
                    },
                    "required":["path","source"],
                    "additionalProperties":false
                }
            },
            "primary_source":{"type":"string","minLength":1,"description":"The path, among files, that holds the kernel entry point."},
            "explanation":{"type":"string","minLength":1,"maxLength":16384,"description":"What this implementation does and every assumption you could not verify from the task artifacts or the admitted claims."}
        },
        "required":["schema_version","files","primary_source","explanation"],
        "additionalProperties":false
    })
}

fn read_task_artifact_native_tool(
    limits: SirTaskLimits,
) -> Result<NativeToolDefinition, MigrationAgentRuntimeError> {
    let max_read_lines = limits.max_read_lines.get();
    let max_read_bytes = limits.max_read_bytes.get();
    Ok(NativeToolDefinition {
        name: MigrationAgentToolV1::ReadTaskArtifact
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description: format!(
            "Read one range from an offered task-local artifact: at most {max_read_lines} lines and {max_read_bytes} UTF-8 source bytes per call. For a longer artifact, make consecutive calls and advance start_line by the number of lines already read."
        ),
        input_schema: json!({
            "type":"object",
            "properties":{
                "schema_version":{"type":"integer","const":1},
                "path":{"type":"string","minLength":1},
                "start_line":{"type":"integer","minimum":1},
                "line_count":{"type":"integer","minimum":1,"maximum":max_read_lines}
            },
            "required":["schema_version","path","start_line","line_count"],
            "additionalProperties":false
        }),
        strict: true,
    })
}

fn sir_native_tools(
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let local_id = json!({
        "type":"string", "minLength":1, "maxLength":64,
        "pattern":"^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$"
    });
    let text_1000 = json!({"type":"string","minLength":1,"maxLength":1000});
    let text_2000 = json!({"type":"string","minLength":1,"maxLength":2000});
    let citation = json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","minLength":1},
            "start_line":{"type":"integer","minimum":1},
            "end_line":{"type":"integer","minimum":1}
        },
        "required":["path","start_line","end_line"], "additionalProperties":false
    });
    let evidence_ref = json!({"oneOf":[
        {"type":"object","properties":{"source":{"type":"string","const":"caller-claim"},"claim":local_id},"required":["source","claim"],"additionalProperties":false},
        {"type":"object","properties":{"source":{"type":"string","const":"observed-fact"},"observation":local_id},"required":["source","observation"],"additionalProperties":false}
    ]});
    let claim_ref = json!({"oneOf":[
        {"type":"object","properties":{"source":{"type":"string","const":"caller-claim"},"claim":local_id},"required":["source","claim"],"additionalProperties":false},
        {"type":"object","properties":{"source":{"type":"string","const":"hypothesis"},"hypothesis":local_id},"required":["source","hypothesis"],"additionalProperties":false}
    ]});
    let target_ref = json!({"oneOf":[
        {"type":"object","properties":{"kind":{"type":"string","const":"hypothesis"},"hypothesis":local_id},"required":["kind","hypothesis"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"type":"string","const":"conflict"},"conflict":local_id},"required":["kind","conflict"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"type":"string","const":"unknown"},"unknown":local_id},"required":["kind","unknown"],"additionalProperties":false}
    ]});
    let observed_fact = json!({
        "type":"object",
        "properties":{"id":local_id,"statement":text_2000,"citations":{"type":"array","minItems":1,"maxItems":8,"items":citation}},
        "required":["id","statement","citations"],"additionalProperties":false
    });
    Ok(vec![
        read_task_artifact_native_tool(limits)?,
        NativeToolDefinition {
            name: MigrationAgentToolV1::SubmitSir
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Submit a cited, competing, non-authoritative intent hypothesis set."
                .to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "observed_facts":{"type":"array","minItems":1,"maxItems":64,"items":observed_fact},
                    "hypotheses":{"type":"array","minItems":2,"maxItems":16,"items":{
                        "type":"object","properties":{
                            "id":local_id,
                            "layer":{"type":"string","enum":["algorithm","numerical","model-deployment","observable-contract"]},
                            "claim":text_2000,"domain":text_1000,
                            "supporting_evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref},
                            "counter_evidence":{"type":"array","maxItems":32,"items":evidence_ref}
                        },"required":["id","layer","claim","domain","supporting_evidence","counter_evidence"],"additionalProperties":false
                    }},
                    "conflicts":{"type":"array","minItems":1,"maxItems":16,"items":{
                        "type":"object","properties":{"id":local_id,"statement":text_2000,"claims":{"type":"array","minItems":2,"maxItems":32,"items":claim_ref},"evidence":{"type":"array","maxItems":32,"items":evidence_ref}},
                        "required":["id","statement","claims","evidence"],"additionalProperties":false
                    }},
                    "unknowns":{"type":"array","minItems":1,"maxItems":32,"items":{
                        "type":"object","properties":{"id":local_id,"kind":{"type":"string","enum":["desired-semantics","source-behavior","numerical-allowance","deployment-context","tool-or-evidence-gap"]},"question":text_2000,"evidence":{"type":"array","maxItems":32,"items":evidence_ref}},
                        "required":["id","kind","question","evidence"],"additionalProperties":false
                    }},
                    "invariants":{"type":"array","minItems":1,"maxItems":32,"items":{
                        "type":"object","properties":{"id":local_id,"statement":text_2000,"evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref}},
                        "required":["id","statement","evidence"],"additionalProperties":false
                    }},
                    "optimization_freedoms":{"type":"array","maxItems":32,"items":{
                        "type":"object","properties":{"id":local_id,"statement":text_2000,"protected_invariants":{"type":"array","minItems":1,"maxItems":32,"items":local_id},"evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref}},
                        "required":["id","statement","protected_invariants","evidence"],"additionalProperties":false
                    }},
                    "source_dispositions":{"type":"array","maxItems":32,"items":{
                        "type":"object","properties":{"id":local_id,"observation":local_id,"disposition":{"type":"string","enum":["preserve-observed-behavior","follow-proposed-semantic-intent","exclude-undefined-region","split-domain","block-pending-user-decision","unknown-classification"]},"rationale":text_2000,"evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref}},
                        "required":["id","observation","disposition","rationale","evidence"],"additionalProperties":false
                    }},
                    "disambiguation_experiments":{"type":"array","maxItems":32,"items":{
                        "type":"object","properties":{"id":local_id,"targets":{"type":"array","minItems":1,"maxItems":32,"items":target_ref},"plan":text_2000,"predictions":{"type":"array","minItems":2,"maxItems":32,"items":text_1000}},
                        "required":["id","targets","plan","predictions"],"additionalProperties":false
                    }}
                },
                "required":["schema_version","observed_facts","hypotheses","conflicts","unknowns","invariants","optimization_freedoms","source_dispositions","disambiguation_experiments"],
                "additionalProperties":false
            }),
            strict: true,
        },
    ])
}

fn oracle_item_discovery_native_tools(
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    Ok(vec![
        read_task_artifact_native_tool(limits)?,
        NativeToolDefinition {
            name: MigrationAgentToolV1::ReadOracleDimension
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Read the one exact Controller-derived Oracle dimension.".to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{"schema_version":{"type":"integer","const":1}},
                "required":["schema_version"],"additionalProperties":false
            }),
            strict: true,
        },
        NativeToolDefinition {
            name: MigrationAgentToolV1::SubmitOracleDimensionItems
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Submit one or more independently reviewable item statements for only the offered dimension.".to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "dimension_id":{"type":"string","minLength":1},
                    "items":{"type":"array","minItems":1,"maxItems":32,"items":text}
                },
                "required":["schema_version","dimension_id","items"],
                "additionalProperties":false
            }),
            strict: true,
        },
    ])
}

/// The one check-plan shape both Oracle authoring roles are offered.
///
/// Both roles submit the same object, so they describe it once. Letting the two drift would mean
/// a plan a model was told to write in one role is rejected by the validator in the other.
fn oracle_check_plan_schema(text: &Value, evidence: &Value) -> Value {
    // The tolerance travels as a binary32 bit pattern: a decimal would be rounded on the way
    // through JSON, and a comparator whose threshold moved in transit is not the one anybody
    // agreed to.
    let allowance = json!({
        "type":"integer","minimum":0,"maximum":4_294_967_295_u32,
        "description":"IEEE-754 binary32 tolerance, given as its unsigned bit pattern."
    });
    let assertion = json!({
        "type":"object",
        "properties":{
            "comparator":{"oneOf":[
                {"type":"object","properties":{"kind":{"type":"string","const":"exact-bytes"}},"required":["kind"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"type":"string","const":"absolute-binary32"},"allowance":allowance},"required":["kind","allowance"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"type":"string","const":"relative-binary32"},"allowance":allowance},"required":["kind","allowance"],"additionalProperties":false}
            ]},
            "allowance_provenance":{
                "type":"string",
                "enum":["caller-declared","measured-noise-floor","derived-from-arithmetic","not-applicable"],
                "description":"Where the tolerance came from. Use not-applicable only with exact-bytes."
            }
        },
        "required":["comparator","allowance_provenance"],
        "additionalProperties":false
    });
    json!({
        "type":"object",
        "properties":{
            "method":{"type":"string","enum":["static-analysis","reference-execution","metamorphic","boundary-probe","runtime-observation"]},
            "objective":text,"setup":text,"observation":text,"pass_condition":text,
            "assertion":assertion,
            "evidence":{"type":"array","minItems":1,"maxItems":16,"items":evidence}
        },
        "required":["method","objective","setup","observation","pass_condition","assertion","evidence"],
        "additionalProperties":false
    })
}

fn oracle_whole_portfolio_native_tools(
    admitted_intent: ContentId<cairn_migration::MigrationIntentContractArtifact>,
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    let citation = json!({
        "type":"object",
        "properties":{"path":{"type":"string","minLength":1},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},
        "required":["path","start_line","end_line"],"additionalProperties":false
    });
    let evidence = json!({"oneOf":[
        {"type":"object","properties":{"source":{"type":"string","const":"source-citation"},"citation":citation},"required":["source","citation"],"additionalProperties":false},
        {"type":"object","properties":{"source":{"type":"string","const":"admitted-intent"},"contract":{"type":"string","const":admitted_intent}},"required":["source","contract"],"additionalProperties":false}
    ]});
    let plan = oracle_check_plan_schema(&text, &evidence);
    let item = json!({
        "type":"object",
        "properties":{
            "statement":text,
            "plans":{"type":"array","minItems":1,"maxItems":16,"items":plan}
        },
        "required":["statement","plans"],
        "additionalProperties":false
    });
    let dimension = json!({
        "type":"object",
        "properties":{
            "dimension_id":{"type":"string","minLength":1},
            "items":{"type":"array","minItems":1,"maxItems":32,"items":item}
        },
        "required":["dimension_id","items"],
        "additionalProperties":false
    });
    Ok(vec![
        read_task_artifact_native_tool(limits)?,
        NativeToolDefinition {
            name: MigrationAgentToolV1::ReadOracleWholePortfolioScope
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Read the exact admitted claims, target context, complete dimension inventory, and revision feedback for one whole-portfolio proposal.".to_owned(),
            input_schema: json!({"type":"object","properties":{"schema_version":{"type":"integer","const":1}},"required":["schema_version"],"additionalProperties":false}),
            strict: true,
        },
        NativeToolDefinition {
            name: MigrationAgentToolV1::SubmitOracleWholePortfolio
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Submit one complete candidate-facing Oracle portfolio covering every offered dimension exactly once without claiming Review or Admission.".to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "authority_id":{"type":"string","minLength":1},
                    "dimensions":{"type":"array","minItems":1,"maxItems":128,"items":dimension}
                },
                "required":["schema_version","authority_id","dimensions"],
                "additionalProperties":false
            }),
            strict: true,
        },
    ])
}

fn oracle_item_set_reviewer_native_tools(
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    let finding = json!({
        "type":"object",
        "properties":{
            "issue":{"type":"string","enum":["incomplete-coverage","overlapping-items","vague-item","out-of-dimension","not-candidate-facing"]},
            "explanation":text,
            "required_change":text
        },
        "required":["issue","explanation","required_change"],
        "additionalProperties":false
    });
    let mut tools = oracle_item_discovery_native_tools(limits)?;
    tools.truncate(1);
    tools.extend([
        NativeToolDefinition {
            name: MigrationAgentToolV1::ReadOracleDimensionItems
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Read one exact dimension item-set proposal under semantic Review."
                .to_owned(),
            input_schema: json!({"type":"object","properties":{"schema_version":{"type":"integer","const":1}},"required":["schema_version"],"additionalProperties":false}),
            strict: true,
        },
        NativeToolDefinition {
            name: MigrationAgentToolV1::SubmitOracleDimensionItemsReview
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Approve the exact decomposition or return actionable completeness and separation findings.".to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "dimension_id":{"type":"string","minLength":1},
                    "proposal_id":{"type":"string","minLength":1},
                    "decision":{"oneOf":[
                        {"type":"object","properties":{"decision":{"type":"string","const":"approved"}},"required":["decision"],"additionalProperties":false},
                        {"type":"object","properties":{"decision":{"type":"string","const":"needs-revision"},"findings":{"type":"array","minItems":1,"maxItems":16,"items":finding}},"required":["decision","findings"],"additionalProperties":false}
                    ]}
                },
                "required":["schema_version","dimension_id","proposal_id","decision"],
                "additionalProperties":false
            }),
            strict: true,
        },
    ]);
    Ok(tools)
}

fn oracle_item_developer_native_tools(
    admitted_intent: ContentId<cairn_migration::MigrationIntentContractArtifact>,
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let mut tools = oracle_item_discovery_native_tools(limits)?;
    tools.truncate(1);
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    let citation = json!({
        "type":"object",
        "properties":{"path":{"type":"string","minLength":1},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},
        "required":["path","start_line","end_line"],"additionalProperties":false
    });
    let evidence = json!({"oneOf":[
        {"type":"object","properties":{"source":{"type":"string","const":"source-citation"},"citation":citation},"required":["source","citation"],"additionalProperties":false},
        {"type":"object","properties":{"source":{"type":"string","const":"admitted-intent"},"contract":{"type":"string","const":admitted_intent}},"required":["source","contract"],"additionalProperties":false}
    ]});
    let plan = oracle_check_plan_schema(&text, &evidence);
    tools.push(NativeToolDefinition {
        name: MigrationAgentToolV1::ReadOracleItemConversation
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description: "Read the exact item and, for revisions, its exact prior draft plus actionable Review and Admission feedback.".to_owned(),
        input_schema: json!({"type":"object","properties":{"schema_version":{"type":"integer","const":1}},"required":["schema_version"],"additionalProperties":false}),
        strict: true,
    });
    tools.push(NativeToolDefinition {
        name: MigrationAgentToolV1::ReadOracleControlDiagnostic
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description: "Read bounded stdout/stderr for one failed control receipt offered by this exact item revision.".to_owned(),
        input_schema: json!({
            "type":"object",
            "properties":{
                "schema_version":{"type":"integer","const":1},
                "receipt":{"type":"string","minLength":1}
            },
            "required":["schema_version","receipt"],
            "additionalProperties":false
        }),
        strict: true,
    });
    tools.push(NativeToolDefinition {
        name: MigrationAgentToolV1::SubmitOracleItemDraft
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description:
            "Submit one exact item draft revision containing one or more cited check plans."
                .to_owned(),
        input_schema: json!({
            "type":"object",
            "properties":{
                "schema_version":{"type":"integer","const":1},
                "item_id":{"type":"string","minLength":1},
                "plans":{"type":"array","minItems":1,"maxItems":16,"items":plan}
            },
            "required":["schema_version","item_id","plans"],"additionalProperties":false
        }),
        strict: true,
    });
    Ok(tools)
}

fn oracle_item_reviewer_native_tools(
    limits: SirTaskLimits,
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    let finding = json!({
        "type":"object",
        "properties":{
            "item_id":{"type":"string","minLength":1},
            "issue":{"type":"string","enum":["unresolved-unknown","concern-mismatch","unsupported-evidence","objective-incomplete","setup-incomplete","observation-unexecutable","pass-condition-ambiguous"]},
            "explanation":text,"required_change":text
        },
        "required":["item_id","issue","explanation","required_change"],"additionalProperties":false
    });
    let mut tools = oracle_item_discovery_native_tools(limits)?;
    tools.truncate(1);
    tools.extend([
        NativeToolDefinition {
            name: MigrationAgentToolV1::ReadOracleItemDraft
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Read the exact Oracle item draft revision under Review.".to_owned(),
            input_schema: json!({"type":"object","properties":{"schema_version":{"type":"integer","const":1}},"required":["schema_version"],"additionalProperties":false}),
            strict: true,
        },
        NativeToolDefinition {
            name: MigrationAgentToolV1::SubmitOracleItemReview
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Approve the exact draft or return one or more exact actionable findings."
                .to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "item_id":{"type":"string","minLength":1},
                    "draft_id":{"type":"string","minLength":1},
                    "decision":{"oneOf":[
                        {"type":"object","properties":{"decision":{"type":"string","const":"approved"}},"required":["decision"],"additionalProperties":false},
                        {"type":"object","properties":{"decision":{"type":"string","const":"needs-revision"},"findings":{"type":"array","minItems":1,"maxItems":16,"items":finding}},"required":["decision","findings"],"additionalProperties":false}
                    ]}
                },
                "required":["schema_version","item_id","draft_id","decision"],"additionalProperties":false
            }),
            strict: true,
        },
    ]);
    Ok(tools)
}

fn oracle_portfolio_coherence_reviewer_native_tools()
-> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    let finding = json!({
        "type":"object",
        "properties":{
            "affected_items":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string","minLength":1}},
            "issue":{"type":"string","enum":["contradictory-items","duplicate-coverage","conflicting-pass-conditions","cross-plane-gap","joint-coverage-gap"]},
            "explanation":text,
            "required_change":text
        },
        "required":["affected_items","issue","explanation","required_change"],
        "additionalProperties":false
    });
    Ok(vec![
        NativeToolDefinition {
            name: MigrationAgentToolV1::ReadOraclePortfolio
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Read the exact mechanically assembled portfolio and its independently approved item drafts.".to_owned(),
            input_schema: json!({"type":"object","properties":{"schema_version":{"type":"integer","const":1}},"required":["schema_version"],"additionalProperties":false}),
            strict: true,
        },
        NativeToolDefinition {
            name: MigrationAgentToolV1::SubmitOraclePortfolioCoherenceReview
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Approve cross-item coherence or return exact affected-item findings without redoing item Review.".to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "portfolio_id":{"type":"string","minLength":1},
                    "decision":{"oneOf":[
                        {"type":"object","properties":{"decision":{"type":"string","const":"approved"}},"required":["decision"],"additionalProperties":false},
                        {"type":"object","properties":{"decision":{"type":"string","const":"needs-revision"},"findings":{"type":"array","minItems":1,"maxItems":32,"items":finding}},"required":["decision","findings"],"additionalProperties":false}
                    ]}
                },
                "required":["schema_version","portfolio_id","decision"],
                "additionalProperties":false
            }),
            strict: true,
        },
    ])
}

#[allow(
    clippy::too_many_lines,
    reason = "the strict native schemas for one role are intentionally defined in one catalog"
)]
fn capability_grant(
    access: &AgentStepAccessV1,
    tools: &[NativeToolDefinition],
) -> Result<cairn_agent::AgentStepCapabilityGrantV1, MigrationAgentRuntimeError> {
    let registrations = access
        .tool_invocation
        .registrations()
        .iter()
        .filter(|registration| tools.iter().any(|tool| tool.name == *registration.name()))
        .cloned()
        .collect();
    cairn_agent::AgentStepCapabilityGrantV1::new(registrations)
        .map_err(MigrationAgentRuntimeError::domain)
}

fn exposed_native_tools(
    access: &AgentStepAccessV1,
    available: &[NativeToolDefinition],
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    access
        .exposure
        .tools
        .entries()
        .iter()
        .map(|entry| {
            if entry.name().as_str() == "migration-run-evidence-experiment" {
                return evidence_experiment_native_tool();
            }
            available
                .iter()
                .find(|tool| tool.name == *entry.name())
                .cloned()
                .ok_or_else(|| {
                    MigrationAgentRuntimeError::MissingNativeTool(entry.name().as_str().to_owned())
                })
        })
        .collect()
}

fn evidence_experiment_native_tool() -> Result<NativeToolDefinition, MigrationAgentRuntimeError> {
    Ok(NativeToolDefinition {
        name: MigrationAgentToolV1::RunEvidenceExperiment
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?,
        description: "Request one Controller-authorized, idempotent evidence experiment on an ordinary capability-matched Worker. The current environment executes program as POSIX /bin/sh source (not Python or another interpreter) with the exact frozen task files but no promised CUDA compiler or GPU. Use it for a bounded executable reference/probe that can discriminate a stated hypothesis; it is not Oracle qualification and its receipt is returned only to this Agent episode."
            .to_owned(),
        input_schema: json!({
            "type":"object",
            "properties":{
                "schema_version":{"type":"integer","const":1},
                "language":{"type":"string","enum":["posix-shell"],"description":"Exact execution language. program must contain POSIX /bin/sh source."},
                "purpose":{"type":"string","minLength":1,"maxLength":1024},
                "program":{"type":"string","minLength":1,"maxLength":32768,"description":"POSIX /bin/sh source executed with the task directory as the current working directory."}
            },
            "required":["schema_version","language","purpose","program"],
            "additionalProperties":false
        }),
        strict: true,
    })
}

fn validate_operation(
    operation: &PreparedToolOperation,
    name: &'static str,
    effect: ToolEffectClass,
) -> Result<(), ToolGatewayError> {
    if operation.tool().as_str() != name
        || operation.implementation_version().as_str() != TOOL_VERSION
        || operation.effect() != effect
    {
        return Err(ToolGatewayError::NotStarted(
            "operation does not match the trusted migration role registration".to_owned(),
        ));
    }
    Ok(())
}

fn decode_arguments<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T, ToolGatewayError> {
    let value = cairn_codec::from_slice(bytes)
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    if cairn_codec::to_vec(&value).map_err(|error| ToolGatewayError::Rejected(error.to_string()))?
        != bytes
    {
        return rejected("tool arguments are not canonical current V1");
    }
    Ok(value)
}

fn rejected<T>(message: &str) -> Result<T, ToolGatewayError> {
    Err(ToolGatewayError::Rejected(message.to_owned()))
}

fn put_json<T: ContentType>(
    content: &mut SqliteContentStore,
    value: &Value,
) -> Result<ContentId<T>, MigrationAgentRuntimeError> {
    let bytes = cairn_codec::to_vec(value).map_err(MigrationAgentRuntimeError::domain)?;
    Ok(content
        .put::<T>(&mut Cursor::new(bytes))
        .map_err(MigrationAgentRuntimeError::domain)?
        .content_id)
}

/// Loads one exact artifact body by its content identity.
fn load_exact<T: ContentType, V: DeserializeOwned>(
    content: &SqliteContentStore,
    identity: ContentId<T>,
) -> Result<V, MigrationAgentRuntimeError> {
    let mut bytes = Vec::new();
    content
        .write_to(&identity, &mut bytes)
        .map_err(MigrationAgentRuntimeError::domain)?;
    cairn_codec::from_slice(&bytes).map_err(MigrationAgentRuntimeError::domain)
}

fn archive_exact<T: ContentType, V: Serialize>(
    content: &mut SqliteContentStore,
    expected: ContentId<T>,
    value: &V,
) -> Result<(), MigrationAgentRuntimeError> {
    let bytes = cairn_codec::to_vec(value).map_err(MigrationAgentRuntimeError::domain)?;
    let archived = content
        .put::<T>(&mut Cursor::new(bytes))
        .map_err(MigrationAgentRuntimeError::domain)?
        .content_id;
    if archived != expected {
        return Err(MigrationAgentRuntimeError::ArtifactBinding);
    }
    Ok(())
}

/// Runtime failure classes intentionally exclude source, prompt, arguments, and model bodies.
#[derive(Debug, Error)]
pub enum MigrationAgentRuntimeError {
    #[error("migration role runtime failed at a domain boundary: {0}")]
    Domain(String),
    #[error("migration role runtime task binding changed")]
    TaskBinding,
    #[error("migration role runtime model binding changed")]
    ModelBinding,
    #[error("migration role model dispatch failed ({class:?}): {diagnostic}")]
    ModelDispatch {
        class: TransportFailureClass,
        diagnostic: String,
    },
    #[error("migration role runtime artifact binding changed")]
    ArtifactBinding,
    #[error("migration role hook exposed tool {0} without a native definition")]
    MissingNativeTool(String),
    #[error("migration role runtime state lock is unavailable")]
    StatePoisoned,
    #[error("migration role runtime has no frozen task {0}")]
    UnknownTask(TaskId),
    #[error("migration role runtime has no frozen Oracle materials")]
    MissingOracleMaterials,
    #[error("migration role runtime has no frozen Candidate materials")]
    MissingCandidateMaterials,
    #[error("migration role runtime completed without a {0} submission")]
    MissingSubmission(&'static str),
    #[error("Oracle item {item} exhausted its revision limit {limit:?}")]
    OracleItemRevisionBudgetExhausted {
        item: ContentId<cairn_migration::OracleItemArtifact>,
        limit: OracleItemRevisionLimit,
    },
    #[error("Oracle dimension {dimension} exhausted its item-discovery revision limit {limit:?}")]
    OracleItemDiscoveryRevisionBudgetExhausted {
        dimension: ContentId<cairn_migration::OracleDimensionArtifact>,
        limit: OracleItemDiscoveryRevisionLimit,
    },
    #[error("migration role runtime received an unexpected external effect from {0}")]
    UnexpectedExternalEffect(&'static str),
    #[error("migration role runtime for {0} is not implemented")]
    RoleNotImplemented(&'static str),
}

impl MigrationAgentRuntimeError {
    fn domain(error: impl std::fmt::Display) -> Self {
        Self::Domain(error.to_string())
    }

    fn episode_driver(error: AgentEpisodeDriverError) -> Self {
        match error {
            AgentEpisodeDriverError::ModelDispatch { class, diagnostic } => {
                Self::ModelDispatch { class, diagnostic }
            }
            error => Self::domain(error),
        }
    }
}

impl crate::MigrationRoleExecutionError for MigrationAgentRuntimeError {
    fn model_dispatch_failure_class(&self) -> Option<TransportFailureClass> {
        match self {
            Self::ModelDispatch { class, .. } => Some(*class),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_dispatch_failure_retains_its_transport_class() {
        let error =
            MigrationAgentRuntimeError::episode_driver(AgentEpisodeDriverError::ModelDispatch {
                class: TransportFailureClass::Ambiguous,
                diagnostic: "response body interrupted".into(),
            });

        assert_eq!(
            crate::MigrationRoleExecutionError::model_dispatch_failure_class(&error),
            Some(TransportFailureClass::Ambiguous)
        );
    }

    #[test]
    fn frozen_role_context_is_present_in_the_model_visible_user_message() {
        let context = json!({
            "schema_version": 1,
            "task_artifacts": [{
                "path": "previously-unknown.cu",
                "line_count": 17,
                "identity": "task-artifact-identity"
            }]
        });

        let message = model_visible_role_user_text("Review exact draft.".into(), &context)
            .expect("model-visible context");

        assert!(message.starts_with("Review exact draft.\n\nFrozen role context:\n"));
        assert!(message.contains("\"path\":\"previously-unknown.cu\""));
        assert!(message.contains("\"line_count\":17"));
    }

    #[test]
    fn task_read_tool_exposes_the_exact_frozen_limits() {
        let limits = SirTaskLimits {
            max_read_lines: SirReadLineLimit::new(512).expect("read line limit"),
            max_read_bytes: cairn_migration::SirReadByteLimit::new(65_536)
                .expect("read byte limit"),
            ..SirTaskLimits::default()
        };

        let tool = read_task_artifact_native_tool(limits).expect("read tool definition");

        assert_eq!(
            tool.input_schema["properties"]["line_count"]["maximum"],
            json!(512)
        );
        assert!(tool.description.contains("at most 512 lines"));
        assert!(tool.description.contains("65536 UTF-8 source bytes"));
        assert!(tool.description.contains("advance start_line"));
    }

    #[test]
    fn the_candidate_exploration_role_is_offered_exactly_its_three_tools() {
        let tools = candidate_exploration_native_tools(SirTaskLimits::default())
            .expect("candidate exploration tools");

        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "migration-read-task-artifact",
                "migration-read-admitted-oracle",
                "migration-submit-candidate",
            ]
        );
        assert!(tools.iter().all(|tool| tool.strict));
    }

    // The offered schema and the domain validator have to describe the same object. The schema is
    // the only description of a submission the model is given, so a field the validator requires
    // and the schema omits is a rejection the model cannot act on.
    #[test]
    fn the_candidate_submit_schema_describes_what_the_domain_type_accepts() {
        let tools = candidate_exploration_native_tools(SirTaskLimits::default())
            .expect("candidate exploration tools");
        let submit = tools
            .iter()
            .find(|tool| tool.name.as_str() == "migration-submit-candidate")
            .expect("submit tool");

        let required: Vec<&str> = submit.input_schema["required"]
            .as_array()
            .expect("required fields")
            .iter()
            .map(|field| field.as_str().expect("field name"))
            .collect();
        assert_eq!(
            required,
            ["schema_version", "files", "primary_source", "explanation"]
        );

        let offered = json!({
            "schema_version": 1,
            "explanation": "Assumption I could not verify: the caller passes contiguous input.",
            "files": [{"path": "kernel.cpp", "source": "// implementation\n"}],
            "primary_source": "kernel.cpp",
        });
        let bytes = cairn_codec::to_vec(&offered).expect("canonical arguments");
        let submission: CandidateProposalSubmissionV1 =
            cairn_codec::from_slice(&bytes).expect("a submission shaped by the schema decodes");
        assert_eq!(submission.files().len(), 1);
    }

    // The schema bounds shape; it cannot express that the entry point has to be one of the files
    // carried. That is the validator's job, and a proposal whose entry point is absent would
    // otherwise reach a build as a source tree with no kernel in it.
    #[test]
    fn a_submission_whose_entry_point_is_not_among_its_files_is_refused() {
        let orphan = json!({
            "schema_version": 1,
            "explanation": "The entry point names a file this proposal does not carry.",
            "files": [{"path": "kernel.cpp", "source": "// implementation\n"}],
            "primary_source": "absent.cpp",
        });
        let bytes = cairn_codec::to_vec(&orphan).expect("canonical arguments");

        assert!(cairn_codec::from_slice::<CandidateProposalSubmissionV1>(&bytes).is_err());
    }

    fn identity<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("identity")
    }

    fn admitted_claim(label: &str) -> AuthoritativeIntentClaimV1 {
        let caller_claim = cairn_migration::SirCallerClaimV1::new(
            cairn_migration::SirCallerClaimId::new(format!("caller-{label}"))
                .expect("caller claim id"),
            cairn_migration::SirIntentLayer::Algorithm,
            cairn_migration::SirCallerClaimStatement::new(format!("caller statement {label}"))
                .expect("caller claim statement"),
            Vec::new(),
        )
        .expect("caller claim");
        AuthoritativeIntentClaimV1::new(
            cairn_migration::OperationIntentV1::new(
                vec![caller_claim],
                cairn_migration::SirIntentLayer::Algorithm,
                cairn_migration::SirHypothesisClaim::new(format!("semantics {label}"))
                    .expect("semantics"),
                cairn_migration::SirIntentDomain::new(format!("domain {label}")).expect("domain"),
            )
            .expect("operation intent"),
        )
    }

    #[test]
    fn exact_dimension_claim_lookup_rejects_a_sibling_claim_projection() {
        let task_id = TaskId::new();
        let admitted_intent =
            identity::<cairn_migration::MigrationIntentContractArtifact>(b"admitted intent");
        let claims = derive_oracle_claims(
            task_id,
            admitted_intent,
            &[admitted_claim("first"), admitted_claim("second")],
        );
        let mut claim_ids = claims
            .iter()
            .map(OracleClaimV1::identity)
            .collect::<Result<Vec<_>, _>>()
            .expect("claim identities");
        claim_ids.sort_by_key(ContentId::to_wire);
        let policy = OracleCoveragePolicyV1::new(
            cairn_migration::OracleCoverageProfileV1::Correctness,
            cairn_migration::OracleAdversarialPolicyV1::NotRequired,
        );
        let dimensions = derive_oracle_dimensions(&claim_ids, &policy).expect("dimensions");
        let scope = whole_portfolio_dimension_scope(&dimensions).expect("dimension scope");
        assert_eq!(scope.len(), dimensions.len());
        for (entry, dimension) in scope.iter().zip(&dimensions) {
            assert_eq!(
                entry["dimension_id"],
                json!(dimension.identity().expect("dimension identity"))
            );
            assert_eq!(entry["dimension"]["claim"], json!(dimension.claim()));
        }
        let second_claim_id = claims[1].identity().expect("second claim identity");
        let dimension = dimensions
            .iter()
            .find(|dimension| dimension.claim() == second_claim_id)
            .expect("second claim dimension");

        assert_eq!(
            exact_oracle_claim_for_dimension(&claims, dimension).expect("exact claim"),
            claims[1]
        );
        assert!(matches!(
            exact_oracle_claim_for_dimension(&claims[..1], dimension),
            Err(MigrationAgentRuntimeError::TaskBinding)
        ));
    }

    #[test]
    fn diagnostic_lookup_rejects_a_receipt_outside_exact_revision_lineage() {
        let offered = identity::<TrustedOracleControlReceiptArtifact>(b"offered receipt");
        let diagnostics = vec![OracleControlDiagnosticMaterialV1 {
            receipt: offered,
            failure_class: OracleControlFailureClassV1::OracleArtifactRejected,
            summary: "honest control rejected the artifact".to_owned(),
            stdout: identity::<ExecutionStdoutArtifact>(b"stdout"),
            stdout_text: "invalid pass condition".to_owned(),
            stderr: identity::<ExecutionStderrArtifact>(b"stderr"),
            stderr_text: String::new(),
        }];

        assert!(offered_control_diagnostic(&diagnostics, offered).is_ok());
        assert!(matches!(
            offered_control_diagnostic(
                &diagnostics,
                identity::<TrustedOracleControlReceiptArtifact>(b"sibling receipt"),
            ),
            Err(ToolGatewayError::Rejected(message))
                if message == "control receipt is not offered by this exact item revision"
        ));
    }

    #[test]
    fn diagnostic_buffer_rejects_content_above_its_exact_bound() {
        let mut output = BoundedDiagnosticBytes {
            bytes: Vec::new(),
            limit: OracleControlDiagnosticReadByteLimit(4),
        };
        assert!(output.write_all(b"four").is_ok());
        assert!(output.write_all(b"!").is_err());
    }

    #[test]
    fn diagnostic_reader_returns_exact_execution_artifact_content() {
        let directory = tempfile::tempdir().expect("temporary content store");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("execution content store");
        let diagnostic = b"pass condition rejected the observed output";
        let artifact = content
            .put::<ExecutionStdoutArtifact>(&mut Cursor::new(diagnostic))
            .expect("diagnostic artifact")
            .content_id;

        assert_eq!(
            read_control_diagnostic_artifact(&content, artifact)
                .expect("read exact diagnostic artifact"),
            String::from_utf8_lossy(diagnostic)
        );
    }
}
