use cairn_agent::{
    AgentContextExposureV1, AgentHookProfileName, AgentHookProfileVersion, AgentLoopContext,
    AgentLoopDirectiveV1, AgentLoopExhaustionReasonV1, AgentLoopHooks, AgentLoopInitializationV1,
    AgentLoopRegistryError, AgentLoopStartV1, AgentRegistries, AgentRoleName, AgentStepAccessV1,
    KnowledgeSourceName, SkillName, ToolName,
};
use cairn_protocol::{ContentId, TaskId};
use serde::Serialize;
use thiserror::Error;

use crate::{
    CandidateAdmissionEvidenceArtifact, CandidateOracleContractArtifact, CandidateProposalArtifact,
    CandidateProposalV1, CandidateWorkspaceArtifact, IntentHypothesisSetProposalV1,
    IntentRecoveryInputArtifact, MigrationIntentContractArtifact, OracleAdmissionOutcomeArtifact,
    OracleDimensionArtifact, OracleDimensionItemSetProposalArtifact,
    OracleDimensionItemSetProposalV1, OracleDimensionItemSetReviewArtifact,
    OracleDimensionItemSetReviewV1, OracleItemArtifact, OracleItemDraftArtifact, OracleItemDraftV1,
    OracleItemReviewArtifact, OracleItemReviewV1, OraclePortfolioCoherenceReviewArtifact,
    OraclePortfolioCoherenceReviewV1, OraclePortfolioProposalArtifact,
    OracleRevisionRequestArtifact, OracleWorkspaceArtifact, SirTaskBundleArtifact,
};

const HOOK_VERSION: &str = "migration-role-hooks-v1";

/// Product tool identities used by role hooks. Registry presence, visibility, and authority are
/// still separate runtime decisions.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MigrationAgentToolV1 {
    ReadTaskArtifact,
    SubmitSir,
    ReadOracleDimension,
    SubmitOracleDimensionItems,
    ReadOracleDimensionItems,
    SubmitOracleDimensionItemsReview,
    ReadOracleItemConversation,
    ReadOracleControlDiagnostic,
    SubmitOracleItemDraft,
    ReadOracleItemDraft,
    SubmitOracleItemReview,
    ReadOraclePortfolio,
    SubmitOraclePortfolioCoherenceReview,
    ReadAdmittedOracle,
    SubmitCandidate,
    SubmitCandidateReview,
    ReadCandidateObservation,
    SubmitCandidateRevision,
}

impl MigrationAgentToolV1 {
    /// Returns the stable task-generic registry name.
    ///
    /// # Errors
    ///
    /// Returns an error only if a built-in constant violates agent label validation.
    pub fn tool_name(self) -> Result<ToolName, MigrationAgentRoleError> {
        ToolName::new(match self {
            Self::ReadTaskArtifact => "migration-read-task-artifact",
            Self::SubmitSir => "migration-submit-sir",
            Self::ReadOracleDimension => "migration-read-oracle-dimension",
            Self::SubmitOracleDimensionItems => "migration-submit-oracle-dimension-items",
            Self::ReadOracleDimensionItems => "migration-read-oracle-dimension-items",
            Self::SubmitOracleDimensionItemsReview => {
                "migration-submit-oracle-dimension-items-review"
            }
            Self::ReadOracleItemConversation => "migration-read-oracle-item-conversation",
            Self::ReadOracleControlDiagnostic => "migration-read-oracle-control-diagnostic",
            Self::SubmitOracleItemDraft => "migration-submit-oracle-item-draft",
            Self::ReadOracleItemDraft => "migration-read-oracle-item-draft",
            Self::SubmitOracleItemReview => "migration-submit-oracle-item-review",
            Self::ReadOraclePortfolio => "migration-read-oracle-portfolio",
            Self::SubmitOraclePortfolioCoherenceReview => {
                "migration-submit-oracle-portfolio-coherence-review"
            }
            Self::ReadAdmittedOracle => "migration-read-admitted-oracle",
            Self::SubmitCandidate => "migration-submit-candidate",
            Self::SubmitCandidateReview => "migration-submit-candidate-review",
            Self::ReadCandidateObservation => "migration-read-candidate-observation",
            Self::SubmitCandidateRevision => "migration-submit-candidate-revision",
        })
        .map_err(|_| MigrationAgentRoleError::InvalidBuiltInLabel)
    }
}

fn context_identity(
    value: &impl Serialize,
) -> Result<ContentId<cairn_agent::AgentLoopContextArtifact>, MigrationAgentRoleError> {
    let bytes = cairn_codec::to_vec(value)
        .map_err(|error| MigrationAgentRoleError::ContextCodec(error.to_string()))?;
    ContentId::derive(&bytes)
        .map_err(|error| MigrationAgentRoleError::ContextCodec(error.to_string()))
}

macro_rules! role_context {
    ($name:ident { $($field:ident : $type:ty),+ $(,)? }) => {
        #[derive(Clone, Debug)]
        pub struct $name {
            task_id: TaskId,
            $($field: $type,)+
            context_id: ContentId<cairn_agent::AgentLoopContextArtifact>,
        }

        impl $name {
            /// Freezes exact typed upstream lineage for one role-scoped Agent Loop.
            ///
            /// # Errors
            ///
            /// Returns an error if the canonical context binding cannot be encoded or identified.
            #[allow(
                clippy::too_many_arguments,
                reason = "role context constructors expose every semantically distinct lineage binding"
            )]
            pub fn new(task_id: TaskId, $($field: $type),+) -> Result<Self, MigrationAgentRoleError> {
                #[derive(Serialize)]
                struct Binding<'a> {
                    schema_version: u16,
                    task_id: TaskId,
                    $($field: &'a $type,)+
                }
                let context_id = context_identity(&Binding {
                    schema_version: 1,
                    task_id,
                    $($field: &$field,)+
                })?;
                Ok(Self { task_id, $($field,)+ context_id })
            }

            #[must_use]
            pub const fn task_id(&self) -> TaskId {
                self.task_id
            }

            $(
                #[must_use]
                pub const fn $field(&self) -> $type {
                    self.$field
                }
            )+
        }

        impl AgentLoopContext for $name {
            fn context_id(&self) -> ContentId<cairn_agent::AgentLoopContextArtifact> {
                self.context_id
            }
        }
    };
}

role_context!(SirAgentContextV1 {
    task_bundle: ContentId<SirTaskBundleArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
});
role_context!(OracleDimensionItemDiscoveryAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    workspace: ContentId<OracleWorkspaceArtifact>,
    dimension: ContentId<OracleDimensionArtifact>,
    previous_item_set: Option<ContentId<OracleDimensionItemSetProposalArtifact>>,
    review_feedback: Option<ContentId<OracleDimensionItemSetReviewArtifact>>,
});
role_context!(OracleDimensionItemSetReviewerAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    dimension: ContentId<OracleDimensionArtifact>,
    proposal: ContentId<OracleDimensionItemSetProposalArtifact>,
});
role_context!(OracleItemDeveloperAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    workspace: ContentId<OracleWorkspaceArtifact>,
    item: ContentId<OracleItemArtifact>,
    previous_draft: Option<ContentId<OracleItemDraftArtifact>>,
    review_feedback: Option<ContentId<OracleItemReviewArtifact>>,
    coherence_feedback: Option<ContentId<OraclePortfolioCoherenceReviewArtifact>>,
    admission_feedback: Option<ContentId<OracleRevisionRequestArtifact>>,
});
role_context!(OracleItemReviewerAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    item: ContentId<OracleItemArtifact>,
    draft: ContentId<OracleItemDraftArtifact>,
});
role_context!(OraclePortfolioCoherenceReviewerAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    portfolio: ContentId<OraclePortfolioProposalArtifact>,
});
role_context!(CandidateExplorationAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    admitted_oracle: ContentId<OracleAdmissionOutcomeArtifact>,
    oracle_contract: ContentId<CandidateOracleContractArtifact>,
    candidate_workspace: ContentId<CandidateWorkspaceArtifact>,
});
role_context!(CandidateReviewAgentContextV1 {
    oracle_contract: ContentId<CandidateOracleContractArtifact>,
    proposal: ContentId<CandidateProposalArtifact>,
});
role_context!(CandidateRevisionAgentContextV1 {
    oracle_contract: ContentId<CandidateOracleContractArtifact>,
    proposal: ContentId<CandidateProposalArtifact>,
    observation: ContentId<CandidateAdmissionEvidenceArtifact>,
});

/// Typed observation returned by a durable role step to its product hook.
pub enum MigrationRoleStepObservationV1<T> {
    Continue,
    Exhausted(AgentLoopExhaustionReasonV1),
    Complete(T),
}

#[derive(Clone, Copy)]
struct RoleAccessPolicy {
    tools: &'static [MigrationAgentToolV1],
}

impl RoleAccessPolicy {
    fn resolve(
        self,
        registries: AgentRegistries<'_>,
    ) -> Result<AgentStepAccessV1, MigrationAgentRoleError> {
        let tools = self
            .tools
            .iter()
            .copied()
            .map(MigrationAgentToolV1::tool_name)
            .collect::<Result<Vec<_>, _>>()?;
        let no_skills: Vec<SkillName> = Vec::new();
        let no_knowledge: Vec<KnowledgeSourceName> = Vec::new();
        Ok(AgentStepAccessV1 {
            exposure: AgentContextExposureV1 {
                tools: registries.tools.index(&tools)?,
                skills: registries.skills.index(&no_skills)?,
                knowledge: registries.knowledge.index(&no_knowledge)?,
            },
            tool_invocation: registries.tools.grant(&tools)?,
            skill_activation: registries.skills.grant(&no_skills)?,
            knowledge_read: registries.knowledge.grant(&no_knowledge)?,
        })
    }
}

macro_rules! role_hooks {
    (
        $hooks:ident,
        context = $context:ty,
        output = $output:ty,
        role = $role:literal,
        profile = $profile:literal,
        tools = [$($tool:path),+ $(,)?]
    ) => {
        pub struct $hooks {
            role: AgentRoleName,
            profile: AgentHookProfileName,
            version: AgentHookProfileVersion,
            access: RoleAccessPolicy,
        }

        impl $hooks {
            /// Constructs the frozen built-in role and hook identity.
            ///
            /// # Errors
            ///
            /// Returns an error if a built-in label violates agent label validation.
            pub fn new() -> Result<Self, MigrationAgentRoleError> {
                Ok(Self {
                    role: AgentRoleName::new($role)
                        .map_err(|_| MigrationAgentRoleError::InvalidBuiltInLabel)?,
                    profile: AgentHookProfileName::new($profile)
                        .map_err(|_| MigrationAgentRoleError::InvalidBuiltInLabel)?,
                    version: AgentHookProfileVersion::new(HOOK_VERSION)
                        .map_err(|_| MigrationAgentRoleError::InvalidBuiltInLabel)?,
                    access: RoleAccessPolicy { tools: &[$($tool),+] },
                })
            }

            #[must_use]
            pub const fn role(&self) -> &AgentRoleName {
                &self.role
            }

            #[must_use]
            pub const fn profile(&self) -> &AgentHookProfileName {
                &self.profile
            }

            #[must_use]
            pub const fn version(&self) -> &AgentHookProfileVersion {
                &self.version
            }
        }

        impl MigrationRoleHooksV1 for $hooks {
            fn role(&self) -> &AgentRoleName {
                &self.role
            }

            fn profile(&self) -> &AgentHookProfileName {
                &self.profile
            }

            fn version(&self) -> &AgentHookProfileVersion {
                &self.version
            }

            fn required_tools(&self) -> &[MigrationAgentToolV1] {
                self.access.tools
            }
        }

        impl AgentLoopHooks<$context> for $hooks {
            type StepObservation = MigrationRoleStepObservationV1<$output>;
            type Output = $output;
            type Error = MigrationAgentRoleError;

            fn profile(&self) -> (&AgentHookProfileName, &AgentHookProfileVersion) {
                (&self.profile, &self.version)
            }

            fn initialize(
                &self,
                start: &AgentLoopStartV1,
                _context: &$context,
                registries: AgentRegistries<'_>,
            ) -> Result<AgentLoopInitializationV1, Self::Error> {
                if start.role() != &self.role {
                    return Err(MigrationAgentRoleError::RoleBindingMismatch);
                }
                Ok(AgentLoopInitializationV1 {
                    first_step_access: self.access.resolve(registries)?,
                })
            }

            fn before_step(
                &self,
                checkpoint: &cairn_agent::AgentLoopCheckpointV1,
                _context: &$context,
                registries: AgentRegistries<'_>,
            ) -> Result<AgentStepAccessV1, Self::Error> {
                if checkpoint.start().role() != &self.role {
                    return Err(MigrationAgentRoleError::RoleBindingMismatch);
                }
                self.access.resolve(registries)
            }

            fn after_step(
                &self,
                _checkpoint: &cairn_agent::AgentLoopCheckpointV1,
                _context: &$context,
                observation: Self::StepObservation,
            ) -> Result<AgentLoopDirectiveV1<Self::Output>, Self::Error> {
                Ok(match observation {
                    MigrationRoleStepObservationV1::Continue => AgentLoopDirectiveV1::Continue,
                    MigrationRoleStepObservationV1::Exhausted(reason) => {
                        AgentLoopDirectiveV1::Exhausted(reason)
                    }
                    MigrationRoleStepObservationV1::Complete(output) => {
                        AgentLoopDirectiveV1::Complete(output)
                    }
                })
            }
        }
    };
}

/// Common frozen identity exposed by every concrete Migration role hook.
pub trait MigrationRoleHooksV1 {
    fn role(&self) -> &AgentRoleName;
    fn profile(&self) -> &AgentHookProfileName;
    fn version(&self) -> &AgentHookProfileVersion;
    fn required_tools(&self) -> &[MigrationAgentToolV1];
}

role_hooks!(
    SirRoleHooksV1,
    context = SirAgentContextV1,
    output = IntentHypothesisSetProposalV1,
    role = "migration-sir-analyst",
    profile = "migration-sir-hooks",
    tools = [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::SubmitSir
    ]
);
role_hooks!(
    OracleDimensionItemSetReviewerRoleHooksV1,
    context = OracleDimensionItemSetReviewerAgentContextV1,
    output = OracleDimensionItemSetReviewV1,
    role = "migration-oracle-item-set-reviewer",
    profile = "migration-oracle-item-set-review-hooks",
    tools = [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::ReadOracleDimensionItems,
        MigrationAgentToolV1::SubmitOracleDimensionItemsReview,
    ]
);
role_hooks!(
    OracleDimensionItemDiscoveryRoleHooksV1,
    context = OracleDimensionItemDiscoveryAgentContextV1,
    output = OracleDimensionItemSetProposalV1,
    role = "migration-oracle-item-discoverer",
    profile = "migration-oracle-item-discovery-hooks",
    tools = [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::ReadOracleDimension,
        MigrationAgentToolV1::SubmitOracleDimensionItems,
    ]
);
role_hooks!(
    OracleItemDeveloperRoleHooksV1,
    context = OracleItemDeveloperAgentContextV1,
    output = OracleItemDraftV1,
    role = "migration-oracle-item-developer",
    profile = "migration-oracle-item-development-hooks",
    tools = [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::ReadOracleItemConversation,
        MigrationAgentToolV1::ReadOracleControlDiagnostic,
        MigrationAgentToolV1::SubmitOracleItemDraft,
    ]
);
role_hooks!(
    OracleItemReviewerRoleHooksV1,
    context = OracleItemReviewerAgentContextV1,
    output = OracleItemReviewV1,
    role = "migration-oracle-item-reviewer",
    profile = "migration-oracle-item-review-hooks",
    tools = [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::ReadOracleItemDraft,
        MigrationAgentToolV1::SubmitOracleItemReview,
    ]
);
role_hooks!(
    OraclePortfolioCoherenceReviewerRoleHooksV1,
    context = OraclePortfolioCoherenceReviewerAgentContextV1,
    output = OraclePortfolioCoherenceReviewV1,
    role = "migration-oracle-portfolio-coherence-reviewer",
    profile = "migration-oracle-portfolio-coherence-review-hooks",
    tools = [
        MigrationAgentToolV1::ReadOraclePortfolio,
        MigrationAgentToolV1::SubmitOraclePortfolioCoherenceReview,
    ]
);
role_hooks!(
    CandidateExplorationRoleHooksV1,
    context = CandidateExplorationAgentContextV1,
    output = CandidateProposalV1,
    role = "migration-candidate-explorer",
    profile = "migration-candidate-exploration-hooks",
    tools = [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::ReadAdmittedOracle,
        MigrationAgentToolV1::SubmitCandidate,
    ]
);
role_hooks!(
    CandidateReviewRoleHooksV1,
    context = CandidateReviewAgentContextV1,
    output = ContentId<CandidateProposalArtifact>,
    role = "migration-candidate-reviewer",
    profile = "migration-candidate-review-hooks",
    tools = [
        MigrationAgentToolV1::ReadAdmittedOracle,
        MigrationAgentToolV1::SubmitCandidateReview,
    ]
);
role_hooks!(
    CandidateRevisionRoleHooksV1,
    context = CandidateRevisionAgentContextV1,
    output = CandidateProposalV1,
    role = "migration-candidate-reviser",
    profile = "migration-candidate-revision-hooks",
    tools = [
        MigrationAgentToolV1::ReadCandidateObservation,
        MigrationAgentToolV1::SubmitCandidateRevision,
    ]
);

#[derive(Debug, Error)]
pub enum MigrationAgentRoleError {
    #[error("built-in migration Agent role label is invalid")]
    InvalidBuiltInLabel,
    #[error("migration Agent Loop role does not match its hook profile")]
    RoleBindingMismatch,
    #[error("migration Agent context identity failed: {0}")]
    ContextCodec(String),
    #[error(transparent)]
    Registry(#[from] AgentLoopRegistryError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use cairn_agent::AgentLoopContext;

    use super::*;

    #[test]
    fn exact_lineage_changes_candidate_revision_context_identity() {
        let task_id = TaskId::new();
        let contract = ContentId::<CandidateOracleContractArtifact>::derive(b"contract")
            .expect("contract identity");
        let proposal =
            ContentId::<CandidateProposalArtifact>::derive(b"proposal").expect("proposal identity");
        let first = CandidateRevisionAgentContextV1::new(
            task_id,
            contract,
            proposal,
            ContentId::<CandidateAdmissionEvidenceArtifact>::derive(b"observation-one")
                .expect("observation identity"),
        )
        .expect("context");
        let second = CandidateRevisionAgentContextV1::new(
            task_id,
            contract,
            proposal,
            ContentId::<CandidateAdmissionEvidenceArtifact>::derive(b"observation-two")
                .expect("observation identity"),
        )
        .expect("context");
        assert_ne!(first.context_id(), second.context_id());
    }

    #[test]
    fn oracle_item_developer_context_binds_exact_feedback_lineage() {
        let task_id = TaskId::new();
        let intent = ContentId::derive(b"intent").expect("intent identity");
        let workspace = ContentId::derive(b"workspace").expect("workspace identity");
        let item = ContentId::derive(b"item").expect("item identity");
        let draft = ContentId::derive(b"draft").expect("draft identity");
        let first_review = ContentId::derive(b"review-one").expect("review identity");
        let second_review = ContentId::derive(b"review-two").expect("review identity");
        let admission = ContentId::derive(b"admission-feedback").expect("admission identity");
        let first = OracleItemDeveloperAgentContextV1::new(
            task_id,
            intent,
            workspace,
            item,
            Some(draft),
            Some(first_review),
            None,
            None,
        )
        .expect("first context");
        let second = OracleItemDeveloperAgentContextV1::new(
            task_id,
            intent,
            workspace,
            item,
            Some(draft),
            Some(second_review),
            None,
            None,
        )
        .expect("second context");
        let admission_revision = OracleItemDeveloperAgentContextV1::new(
            task_id,
            intent,
            workspace,
            item,
            Some(draft),
            None,
            None,
            Some(admission),
        )
        .expect("Admission revision context");

        assert_ne!(first.context_id(), second.context_id());
        assert_ne!(first.context_id(), admission_revision.context_id());
        assert_eq!(first.item(), item);
        assert_eq!(first.previous_draft(), Some(draft));
        assert_eq!(first.review_feedback(), Some(first_review));
        assert_eq!(first.admission_feedback(), None);
        assert_eq!(admission_revision.admission_feedback(), Some(admission));
    }

    #[test]
    fn every_role_has_distinct_identity_and_context_selected_tools() {
        let sir = SirRoleHooksV1::new().expect("SIR hooks");
        let item_discovery =
            OracleDimensionItemDiscoveryRoleHooksV1::new().expect("item discovery hooks");
        let item_set_reviewer =
            OracleDimensionItemSetReviewerRoleHooksV1::new().expect("item-set reviewer hooks");
        let item_developer = OracleItemDeveloperRoleHooksV1::new().expect("item developer hooks");
        let item_reviewer = OracleItemReviewerRoleHooksV1::new().expect("item reviewer hooks");
        let candidate = CandidateExplorationRoleHooksV1::new().expect("Candidate hooks");
        let roles = [
            MigrationRoleHooksV1::role(&sir).to_string(),
            MigrationRoleHooksV1::role(&item_discovery).to_string(),
            MigrationRoleHooksV1::role(&item_developer).to_string(),
            MigrationRoleHooksV1::role(&item_reviewer).to_string(),
            MigrationRoleHooksV1::role(&candidate).to_string(),
        ];
        assert_eq!(roles.iter().collect::<BTreeSet<_>>().len(), roles.len());
        assert_eq!(
            MigrationRoleHooksV1::required_tools(&sir),
            &[
                MigrationAgentToolV1::ReadTaskArtifact,
                MigrationAgentToolV1::SubmitSir,
            ]
        );
        assert!(
            MigrationRoleHooksV1::required_tools(&item_reviewer)
                .contains(&MigrationAgentToolV1::ReadTaskArtifact)
        );
        assert!(
            MigrationRoleHooksV1::required_tools(&item_developer)
                .contains(&MigrationAgentToolV1::ReadOracleControlDiagnostic)
        );
        assert!(
            MigrationRoleHooksV1::required_tools(&item_set_reviewer)
                .contains(&MigrationAgentToolV1::ReadTaskArtifact)
        );
        assert!(
            MigrationRoleHooksV1::required_tools(&candidate)
                .contains(&MigrationAgentToolV1::ReadAdmittedOracle)
        );
    }
}
