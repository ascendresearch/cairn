use cairn_agent::{
    AgentContextExposureV1, AgentHookProfileName, AgentHookProfileVersion, AgentLoopContext,
    AgentLoopDirectiveV1, AgentLoopHooks, AgentLoopInitializationV1, AgentLoopRegistryError,
    AgentLoopStartV1, AgentRegistries, AgentRoleName, AgentStepAccessV1, KnowledgeSourceName,
    SkillName, ToolName,
};
use cairn_protocol::{ContentId, TaskId};
use serde::Serialize;
use thiserror::Error;

use crate::{
    CandidateAdmissionEvidenceArtifact, CandidateOracleContractArtifact, CandidateProposalArtifact,
    CandidateProposalV1, CandidateWorkspaceArtifact, IntentHypothesisSetProposalV1,
    IntentRecoveryInputArtifact, MigrationIntentContractArtifact, OracleAdmissionEvidenceArtifact,
    OracleAdmissionOutcomeArtifact, OraclePortfolioProposalArtifact, OraclePortfolioProposalV1,
    OracleWorkspaceArtifact, SirTaskBundleArtifact,
};

const HOOK_VERSION: &str = "migration-role-hooks-v1";

/// Product tool identities used by role hooks. Registry presence, visibility, and authority are
/// still separate runtime decisions.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MigrationAgentToolV1 {
    ReadTaskArtifact,
    SubmitSir,
    ReadOracleWorkspace,
    SearchExternalTests,
    RequestOracleExperiment,
    SubmitOraclePortfolio,
    SubmitOracleReview,
    ReadOracleControlEvidence,
    SubmitOracleRevision,
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
            Self::ReadOracleWorkspace => "migration-read-oracle-workspace",
            Self::SearchExternalTests => "migration-search-external-tests",
            Self::RequestOracleExperiment => "migration-request-oracle-experiment",
            Self::SubmitOraclePortfolio => "migration-submit-oracle-portfolio",
            Self::SubmitOracleReview => "migration-submit-oracle-review",
            Self::ReadOracleControlEvidence => "migration-read-oracle-control-evidence",
            Self::SubmitOracleRevision => "migration-submit-oracle-revision",
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
role_context!(OracleExplorationAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    workspace: ContentId<OracleWorkspaceArtifact>,
});
role_context!(OracleReviewAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
});
role_context!(OracleRevisionAgentContextV1 {
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    control_evidence: ContentId<OracleAdmissionEvidenceArtifact>,
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
    OracleExplorationRoleHooksV1,
    context = OracleExplorationAgentContextV1,
    output = OraclePortfolioProposalV1,
    role = "migration-oracle-explorer",
    profile = "migration-oracle-exploration-hooks",
    tools = [
        MigrationAgentToolV1::ReadTaskArtifact,
        MigrationAgentToolV1::ReadOracleWorkspace,
        MigrationAgentToolV1::SearchExternalTests,
        MigrationAgentToolV1::RequestOracleExperiment,
        MigrationAgentToolV1::SubmitOraclePortfolio,
    ]
);
role_hooks!(
    OracleReviewRoleHooksV1,
    context = OracleReviewAgentContextV1,
    output = ContentId<OraclePortfolioProposalArtifact>,
    role = "migration-oracle-reviewer",
    profile = "migration-oracle-review-hooks",
    tools = [
        MigrationAgentToolV1::ReadOracleWorkspace,
        MigrationAgentToolV1::SubmitOracleReview,
    ]
);
role_hooks!(
    OracleRevisionRoleHooksV1,
    context = OracleRevisionAgentContextV1,
    output = OraclePortfolioProposalV1,
    role = "migration-oracle-reviser",
    profile = "migration-oracle-revision-hooks",
    tools = [
        MigrationAgentToolV1::ReadOracleWorkspace,
        MigrationAgentToolV1::ReadOracleControlEvidence,
        MigrationAgentToolV1::SubmitOracleRevision,
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
    fn every_role_has_distinct_identity_and_context_selected_tools() {
        let sir = SirRoleHooksV1::new().expect("SIR hooks");
        let oracle = OracleExplorationRoleHooksV1::new().expect("Oracle hooks");
        let oracle_review = OracleReviewRoleHooksV1::new().expect("Oracle review hooks");
        let candidate = CandidateExplorationRoleHooksV1::new().expect("Candidate hooks");
        let roles = [
            MigrationRoleHooksV1::role(&sir).to_string(),
            MigrationRoleHooksV1::role(&oracle).to_string(),
            MigrationRoleHooksV1::role(&oracle_review).to_string(),
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
            MigrationRoleHooksV1::required_tools(&oracle)
                .contains(&MigrationAgentToolV1::RequestOracleExperiment)
        );
        assert!(
            !MigrationRoleHooksV1::required_tools(&oracle_review)
                .contains(&MigrationAgentToolV1::RequestOracleExperiment)
        );
        assert!(
            MigrationRoleHooksV1::required_tools(&candidate)
                .contains(&MigrationAgentToolV1::ReadAdmittedOracle)
        );
    }
}
