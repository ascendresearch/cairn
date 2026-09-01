use std::{
    collections::BTreeMap,
    future::{Future, ready},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use cairn_execution::{ExecutionStderrArtifact, ExecutionStdoutArtifact};

use cairn_agent::{
    AgentLoopCheckpointV1, AgentLoopExhaustionReasonV1, AgentLoopStepExecutionV1,
    AgentLoopStepExecutor, AgentStepAccessV1, CanonicalToolResult, ContextBlock, EpisodeBudget,
    FrozenAgentEpisodeDriverV1, HistoryItem, HttpModelTransport, InstructionBlock,
    ModelOutputTokenLimit, ModelSelection, NativeProtocolCodec, NativeRequestSpec,
    NativeToolDefinition, PolicyDocument, PreparedToolOperation, ResolvedRuntimeModel, ToolCatalog,
    ToolDescriptorArtifact, ToolEffectClass, ToolGateway, ToolGatewayError,
    ToolImplementationVersion, ToolRegistration, ToolRegistry, drive_agent_episode_step,
};
use cairn_migration::{
    AgentLoopRuntimeBindingArtifact, AgentResolvedRuntimeModelArtifact, AuthoritativeIntentClaimV1,
    CandidateExplorationAgentContextV1, CandidateProposalV1, CandidateReviewAgentContextV1,
    CandidateRevisionAgentContextV1, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    MigrationAgentToolV1, MigrationRoleStepObservationV1, OracleBuildTestSnapshotArtifact,
    OracleCheckEvidenceV1, OracleCheckMethodV1, OracleCheckObjective, OracleCheckObservation,
    OracleCheckPassCondition, OracleCheckPlanV1, OracleCheckSetup, OracleControlFailureClassV1,
    OracleControlResultV1, OracleCoveragePolicyV1, OracleDimensionItemDiscoveryAgentContextV1,
    OracleDimensionItemSetProposalV1, OracleDimensionItemSetReviewDecisionV1,
    OracleDimensionItemSetReviewV1, OracleDimensionItemSetReviewerAgentContextV1,
    OracleDimensionV1, OracleDocumentationSnapshotArtifact, OracleExperimentLimit,
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
    OracleStrategyRunV1, OracleStrategyToolCatalogV1, OracleWorkspaceInput, OracleWorkspaceV1,
    SirAgentContextV1, SirProposalSubmissionV1, SirReadLineLimit, SirSourceLineNumber,
    SirTaskArtifactPath, SirTaskLimits, SirTaskWorkspace, TrustedOracleControlReceiptArtifact,
    derive_oracle_claims, derive_oracle_dimensions,
};
use cairn_protocol::{AgentLoopId, ContentId, ContentType, EpisodeId, TaskId};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_verification::ModelConfigurationArtifact;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;

const SCHEMA_V1: u16 = 1;
const TOOL_VERSION: &str = "migration-role-tools-v1";
const SIR_INSTRUCTION: &str = r"You are the semantic-intent-recovery analyst for one CUDA-to-Ascend-C migration task.

Inspect only the offered task artifacts. First use migration-read-task-artifact to read the source, host launch, ABI, tests, or build files needed for your analysis. Treat observable source facts separately from intent inferences. Cite exact task-local paths and inclusive line ranges.

The caller declaration is an attributed authority source, not a fact to overwrite. Keep caller claims separate from source observations and from your hypotheses. Submit exactly one complete proposal through migration-submit-sir. It must contain source-observed facts with citations, at least two genuinely competing hypotheses, an explicit conflict, at least one unknown, and at least one evidence-backed invariant. When a conflict cannot be resolved by offered evidence and requires the task authority, include an unknown for that exact decision and a disambiguation experiment that jointly targets the unknown plus either the conflict itself or at least two hypotheses named by that conflict. Classify the unknown by its real subject. Also report applicable optimization freedoms, source-behavior dispositions, and disambiguation experiments; use empty arrays when none are justified. Every reference must point to an ID declared in this proposal or the frozen caller declaration. Use lowercase kebab-case local IDs and sort every top-level collection lexicographically by its id.

The proposal is non-authoritative. Do not claim admission, correctness, a confidence score, or a migration verdict. Do not invent content identities or use paths outside the offered task bundle.";

const ORACLE_ITEM_DISCOVERY_INSTRUCTION: &str = r"You discover independently reviewable Oracle items for one exact Controller-derived dimension.

Read the offered dimension and task evidence. Decompose the dimension into one or more distinct, concrete obligations that together express what this dimension needs checked. On a revision, read and address every exact item-set Review finding without changing the dimension. Submit only item statements through migration-submit-oracle-dimension-items. Do not design check plans yet, merge dimensions, invent identities, claim review or admission, or use unavailable knowledge.";

const ORACLE_ITEM_SET_REVIEW_INSTRUCTION: &str = r"You independently review one exact proposed item decomposition for one Controller-derived Oracle dimension.

Read migration-read-oracle-dimension-items, including the full Controller-derived dimension. Use migration-read-task-artifact for the exact source ranges needed to check whether the decomposition is grounded and complete. Approve only if the items are concrete, non-overlapping, remain inside the exact dimension, and jointly cover that dimension. Reject unsupported or unreadable evidence with actionable findings through migration-submit-oracle-dimension-items-review. Do not design check plans, rewrite the item set yourself, or claim control or Admission authority.";

const ORACLE_ITEM_DEVELOPMENT_INSTRUCTION: &str = r"You develop one exact Oracle item for a CUDA-to-Ascend-C migration task.

Read migration-read-oracle-item-conversation before submitting. On the initial revision, create one or more complementary, executable check plans for only the offered item. On a later revision, preserve the same item and address every finding from the exact prior draft review and every exact artifact-owned failed control supplied by Admission. For each failed receipt, call migration-read-oracle-control-diagnostic and use its bounded exact stdout/stderr to determine the required correction; do not guess from an exit code or artifact identity. Treat prior receipts only as feedback, never as passing authority. Negative-challenge, mechanism, infrastructure-unavailable, or missing observations are reconciled by the Controller and are never a reason to rewrite an item. Each plan must state an objective, setup, obtainable observation, unambiguous pass condition, and exact source citation or admitted-intent evidence. Submit only through migration-submit-oracle-item-draft. Do not change the item, omit feedback, claim execution, review, qualification, or admission.";

const ORACLE_ITEM_REVIEW_INSTRUCTION: &str = r"You independently review one exact Oracle item draft revision.

Read migration-read-oracle-item-draft. Use migration-read-task-artifact to inspect every exact source range needed to verify the draft's source citations; never treat an unread citation as support. Approve only if every proposed plan addresses the exact item, is supported by readable cited evidence, has complete setup, an obtainable observation, and an unambiguous pass condition. Otherwise submit one or more actionable findings bound to this exact item and draft through migration-submit-oracle-item-review. Multiple distinct findings may use the same issue class. Do not redesign the plan yourself or claim qualification or admission.";

const ORACLE_PORTFOLIO_COHERENCE_REVIEW_INSTRUCTION: &str = r"You independently review only the relationships among already item-reviewed Oracle drafts in one exact portfolio.

Read migration-read-oracle-portfolio. Check for contradictory items, duplicate coverage, conflicting pass conditions, cross-plane gaps, and failures of the items to provide coherent joint coverage. Do not redo each item's detailed plan review and do not generate plans. Approve only when the exact assembled portfolio is coherent. Otherwise submit actionable findings through migration-submit-oracle-portfolio-coherence-review; every finding must name a non-empty exact affected item set, an issue class, an explanation, and a required change. You have no control, receipt, or Admission authority.";

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
    workspace: SirTaskWorkspace,
    recovery_input: IntentRecoveryInputV1,
    limits: SirTaskLimits,
    oracle: Option<RuntimeOracleTaskV1>,
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
            {
                return Err(MigrationAgentRuntimeError::TaskBinding);
            }
            return Ok(());
        }
        tasks.insert(
            task_id,
            RuntimeTaskV1 {
                workspace,
                recovery_input,
                limits,
                oracle: None,
            },
        );
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
    let claims = derive_oracle_claims(
        task_id,
        oracle.workspace.admitted_intent(),
        &oracle.admitted_claims,
    );
    let mut claim_ids = claims
        .iter()
        .map(cairn_migration::OracleClaimV1::identity)
        .collect::<Result<Vec<_>, _>>()
        .map_err(MigrationAgentRuntimeError::domain)?;
    claim_ids.sort_by_key(ContentId::to_wire);
    derive_oracle_dimensions(&claim_ids, &oracle.policy).map_err(MigrationAgentRuntimeError::domain)
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
    item_set_submissions: BTreeMap<AgentLoopId, OracleDimensionItemSetProposalV1>,
    item_set_review_submissions: BTreeMap<AgentLoopId, OracleDimensionItemSetReviewV1>,
    item_draft_submissions: BTreeMap<AgentLoopId, OracleItemDraftV1>,
    item_review_submissions: BTreeMap<AgentLoopId, OracleItemReviewV1>,
    coherence_review_submissions: BTreeMap<AgentLoopId, OraclePortfolioCoherenceReviewV1>,
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
            item_set_submissions: BTreeMap::new(),
            item_set_review_submissions: BTreeMap::new(),
            item_draft_submissions: BTreeMap::new(),
            item_review_submissions: BTreeMap::new(),
            coherence_review_submissions: BTreeMap::new(),
        })
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
                OracleStrategyName::new("model-backed-synthesis")
                    .map_err(MigrationAgentRuntimeError::domain)?,
                OracleStrategyKindV1::ModelBackedSynthesis,
                OracleStrategyExecutorV1::AgentLoop {
                    authorship_model,
                    invocation,
                    tools,
                },
                vec![OracleStrategyRoleV1::Synthesis],
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
        let tools = exposed_native_tools(access, &oracle_item_discovery_native_tools()?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "workspace_id": context.workspace(),
            "admitted_intent_id": context.admitted_intent(),
            "dimension_id": dimension_id,
            "previous_item_set_id": context.previous_item_set(),
            "review_feedback_id": context.review_feedback(),
            "task_artifacts": task.workspace.bundle().artifacts(),
            "knowledge_snapshot": {"kind":"empty"},
        });
        let projection = archive_oracle_item_projection(
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
                instructions: ORACLE_ITEM_DISCOVERY_INSTRUCTION.to_owned(),
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
        .map_err(MigrationAgentRuntimeError::domain)?;
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
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(_) => Err(
                MigrationAgentRuntimeError::UnexpectedExternalEffect("Oracle item discovery"),
            ),
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
        let tools = exposed_native_tools(access, &oracle_item_set_reviewer_native_tools()?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "admitted_intent_id": context.admitted_intent(),
            "dimension_id": context.dimension(),
            "proposal_id": context.proposal(),
            "revision": proposal.revision().get(),
        });
        let projection = archive_oracle_item_projection(
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
                instructions: ORACLE_ITEM_SET_REVIEW_INSTRUCTION.to_owned(),
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
        .map_err(MigrationAgentRuntimeError::domain)?;
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
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(_) => Err(
                MigrationAgentRuntimeError::UnexpectedExternalEffect("Oracle item-set Review"),
            ),
        }
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
            OracleStrategyName::new("model-backed-synthesis")
                .map_err(MigrationAgentRuntimeError::domain)?,
            &oracle.catalog,
        )
        .map_err(MigrationAgentRuntimeError::domain)?;
        let run_id = run.identity().map_err(MigrationAgentRuntimeError::domain)?;
        archive_exact(&mut self.content, run_id, &run)?;
        let tools = exposed_native_tools(
            access,
            &oracle_item_developer_native_tools(context.admitted_intent())?,
        )?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "workspace_id": context.workspace(),
            "admitted_intent_id": context.admitted_intent(),
            "item_id": context.item(),
            "previous_draft_id": context.previous_draft(),
            "review_feedback_id": context.review_feedback(),
            "coherence_feedback_id": context.coherence_feedback(),
            "admission_feedback_id": context.admission_feedback(),
            "task_artifacts": task.workspace.bundle().artifacts(),
            "knowledge_snapshot": {"kind":"empty"},
        });
        let projection = archive_oracle_item_projection(
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
                instructions: ORACLE_ITEM_DEVELOPMENT_INSTRUCTION.to_owned(),
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
        .map_err(MigrationAgentRuntimeError::domain)?;
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
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(_) => Err(
                MigrationAgentRuntimeError::UnexpectedExternalEffect("Oracle item development"),
            ),
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
        let tools = exposed_native_tools(access, &oracle_item_reviewer_native_tools()?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "admitted_intent_id": context.admitted_intent(),
            "item_id": context.item(),
            "draft_id": context.draft(),
            "revision": draft.revision().get(),
        });
        let projection = archive_oracle_item_projection(
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
                instructions: ORACLE_ITEM_REVIEW_INSTRUCTION.to_owned(),
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
        .map_err(MigrationAgentRuntimeError::domain)?;
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
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(_) => Err(
                MigrationAgentRuntimeError::UnexpectedExternalEffect("Oracle item Review"),
            ),
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
        let tools =
            exposed_native_tools(access, &oracle_portfolio_coherence_reviewer_native_tools()?)?;
        let model_context = json!({
            "schema_version": SCHEMA_V1,
            "admitted_intent_id": context.admitted_intent(),
            "portfolio_id": context.portfolio(),
            "item_count": portfolio.accepted_items().len(),
        });
        let projection = archive_oracle_item_projection(
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
                instructions: ORACLE_PORTFOLIO_COHERENCE_REVIEW_INSTRUCTION.to_owned(),
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
        .map_err(MigrationAgentRuntimeError::domain)?;
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
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(_) => {
                Err(MigrationAgentRuntimeError::UnexpectedExternalEffect(
                    "Oracle portfolio coherence Review",
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
        let tools = exposed_native_tools(access, &sir_native_tools()?)?;
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
                instructions: SIR_INSTRUCTION.to_owned(),
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
        .map_err(MigrationAgentRuntimeError::domain)?;
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
            cairn_agent::AgentEpisodeDriverStepOutcomeV1::WorkerRequest(_) => {
                Err(MigrationAgentRuntimeError::UnexpectedExternalEffect("SIR"))
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

macro_rules! unavailable_role_executor {
    ($context:ty, $output:ty, $role:literal) => {
        impl AgentLoopStepExecutor<$context, MigrationRoleStepObservationV1<$output>>
            for MigrationAgentRuntimeExecutorV1
        {
            type Error = MigrationAgentRuntimeError;

            fn execute_step(
                &mut self,
                _checkpoint: &AgentLoopCheckpointV1,
                _context: &$context,
                _access: &AgentStepAccessV1,
            ) -> impl Future<
                Output = Result<
                    AgentLoopStepExecutionV1<MigrationRoleStepObservationV1<$output>>,
                    Self::Error,
                >,
            > + Send {
                ready(Err(MigrationAgentRuntimeError::RoleNotImplemented($role)))
            }
        }
    };
}

unavailable_role_executor!(
    CandidateExplorationAgentContextV1,
    CandidateProposalV1,
    "Candidate Exploration"
);
unavailable_role_executor!(
    CandidateReviewAgentContextV1,
    ContentId<cairn_migration::CandidateProposalArtifact>,
    "Candidate Review"
);
unavailable_role_executor!(
    CandidateRevisionAgentContextV1,
    CandidateProposalV1,
    "Candidate Revision"
);

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
                    "claim_id": self.dimension.claim(),
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
    evidence: Vec<OracleSubmittedCheckEvidenceV1>,
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
        MigrationAgentToolV1::SubmitSir,
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
        MigrationAgentToolV1::SubmitCandidateReview,
        MigrationAgentToolV1::ReadCandidateObservation,
        MigrationAgentToolV1::SubmitCandidateRevision,
    ] {
        let name = tool
            .tool_name()
            .map_err(MigrationAgentRuntimeError::domain)?;
        let effect = match tool {
            MigrationAgentToolV1::ReadTaskArtifact
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

fn archive_oracle_item_projection(
    content: &mut SqliteContentStore,
    instruction_text: &str,
    tools: &[NativeToolDefinition],
    model_context: &Value,
    user_text: String,
) -> Result<SirProjectionV1, MigrationAgentRuntimeError> {
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
    if request.schema_version != SCHEMA_V1 || request.line_count.get() > limits.max_read_lines.get()
    {
        return rejected("task read violates the current V1 limit");
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
        return rejected("task read exceeds the current V1 byte limit");
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
fn sir_native_tools() -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
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
        NativeToolDefinition {
            name: MigrationAgentToolV1::ReadTaskArtifact
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Read a bounded line range from one offered task-local artifact."
                .to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "path":{"type":"string","minLength":1},
                    "start_line":{"type":"integer","minimum":1},
                    "line_count":{"type":"integer","minimum":1,"maximum":200}
                },
                "required":["schema_version","path","start_line","line_count"],
                "additionalProperties":false
            }),
            strict: true,
        },
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

fn oracle_item_discovery_native_tools()
-> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    Ok(vec![
        NativeToolDefinition {
            name: MigrationAgentToolV1::ReadTaskArtifact
                .tool_name()
                .map_err(MigrationAgentRuntimeError::domain)?,
            description: "Read a bounded line range from one offered task-local artifact."
                .to_owned(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "schema_version":{"type":"integer","const":1},
                    "path":{"type":"string","minLength":1},
                    "start_line":{"type":"integer","minimum":1},
                    "line_count":{"type":"integer","minimum":1,"maximum":200}
                },
                "required":["schema_version","path","start_line","line_count"],
                "additionalProperties":false
            }),
            strict: true,
        },
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

fn oracle_item_set_reviewer_native_tools()
-> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    let finding = json!({
        "type":"object",
        "properties":{
            "issue":{"type":"string","enum":["incomplete-coverage","overlapping-items","vague-item","out-of-dimension"]},
            "explanation":text,
            "required_change":text
        },
        "required":["issue","explanation","required_change"],
        "additionalProperties":false
    });
    let mut tools = oracle_item_discovery_native_tools()?;
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
) -> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
    let mut tools = oracle_item_discovery_native_tools()?;
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
    let plan = json!({
        "type":"object",
        "properties":{
            "method":{"type":"string","enum":["static-analysis","reference-execution","metamorphic","boundary-probe","runtime-observation"]},
            "objective":text,"setup":text,"observation":text,"pass_condition":text,
            "evidence":{"type":"array","minItems":1,"maxItems":16,"items":evidence}
        },
        "required":["method","objective","setup","observation","pass_condition","evidence"],
        "additionalProperties":false
    });
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

fn oracle_item_reviewer_native_tools()
-> Result<Vec<NativeToolDefinition>, MigrationAgentRuntimeError> {
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
    let mut tools = oracle_item_discovery_native_tools()?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("identity")
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
