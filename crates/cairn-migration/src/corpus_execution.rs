//! Complete executable-corpus planning over the generic execution boundary.

use std::collections::{BTreeMap, BTreeSet};

use cairn_execution::{ExecutionEnvironmentArtifact, InputBundleArtifact, JobContractArtifact};
use cairn_protocol::{ContentId, ContentType, JobId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AssembledBoundaryCaseInput, AssembledInputValueCaseInput, AssembledMemorySurfaceCaseInput,
    CallAdapterCaptureLimits, CallAdapterExecutableArtifact, CallAdapterExecutableByteLimit,
    CallAdapterProtocolError, CallAdapterRequestArtifact, CaseExpectedOutcome,
    CorpusInvocationIdentityV1, InputValueDisposition, InvalidInputBehavior,
    MandatoryInputValueCaseArtifact, MandatoryInputValueCasesArtifact, MandatoryInputValueCasesV1,
    MandatoryMemorySurfaceCaseArtifact, MandatoryMemorySurfaceCasesArtifact,
    MandatoryMemorySurfaceCasesV1, MemoryConditionDisposition, MigrationDomainCaseArtifact,
    MigrationExecutionNeed, MigrationMandatoryCasesArtifact, MigrationMandatoryCasesV1,
    MigrationValidationTier, PreparedCallAdapterInput, PreparedCallAdapterJob,
    compose_call_adapter_job, prepare_boundary_call_adapter_input,
    prepare_input_value_call_adapter_input, prepare_memory_surface_call_adapter_input,
};
use cairn_verification::CallerDomainBodyArtifact;
use cairn_verification::{
    ImplementationBundleArtifact, ImplementationVariantArtifact, PropertyRelationArtifact,
    ReferenceArtifact,
};

/// Failure to turn the complete executable obligation surface into isolated jobs.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CorpusExecutionPlanError {
    /// Only the current pre-release V1 plan is accepted.
    #[error("corpus execution plan schema version must be 1")]
    UnsupportedSchemaVersion,
    /// Mandatory obligation sets do not cite one caller domain.
    #[error("mandatory corpus obligation domains differ")]
    ObligationDomainMismatch,
    /// One executable obligation was supplied more than once.
    #[error("duplicate assembled corpus case: {obligation:?}")]
    DuplicateCase {
        obligation: CorpusObligationIdentityV1,
    },
    /// One executable mandatory obligation has no assembled case.
    #[error("missing assembled corpus case: {obligation:?}")]
    MissingCase {
        obligation: CorpusObligationIdentityV1,
    },
    /// An assembled case is not an executable member of the cited mandatory sets.
    #[error("unexpected assembled corpus case: {obligation:?}")]
    UnexpectedCase {
        obligation: CorpusObligationIdentityV1,
    },
    /// An assembled case cites another domain or contradicts its mandatory expectation.
    #[error("assembled corpus case contradicts its obligation")]
    InconsistentCase,
    /// Persisted plan collections or cross-field bindings are contradictory.
    #[error("corpus execution plan is inconsistent")]
    InconsistentPlan,
    /// Adapter input preparation or generic job composition failed.
    #[error("corpus adapter job preparation failed: {message}")]
    Adapter { message: String },
    /// Canonical encoding or typed identity derivation failed.
    #[error("corpus execution plan codec error: {message}")]
    Codec { message: String },
}

/// Strong identity of the mandatory obligation realized by one executable invocation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CorpusObligationIdentityV1 {
    /// Quantitative boundary obligation.
    Boundary {
        case: ContentId<MigrationDomainCaseArtifact>,
    },
    /// Dtype-pattern obligation.
    InputValue {
        case: ContentId<MandatoryInputValueCaseArtifact>,
    },
    /// Pointer/capacity/aliasing obligation.
    MemorySurface {
        case: ContentId<MandatoryMemorySurfaceCaseArtifact>,
    },
}

/// Upstream artifact role represented by one complete executable corpus plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CorpusExecutionSubjectV1 {
    /// Caller/source implementation being interrogated during oracle admission.
    Source {
        implementation: ContentId<ImplementationBundleArtifact>,
    },
    /// Proposed semantic reference or allowed-result-set implementation.
    Reference {
        reference: ContentId<ReferenceArtifact>,
    },
    /// Proposed property or metamorphic relation implementation.
    Property {
        property: ContentId<PropertyRelationArtifact>,
    },
    /// Correct-by-construction or deliberately incorrect admission variant.
    AdmissionVariant {
        variant: ContentId<ImplementationVariantArtifact>,
    },
    /// Candidate implementation whose observations may later be judged by an admitted oracle.
    Candidate {
        implementation: ContentId<ImplementationBundleArtifact>,
    },
}

impl CorpusObligationIdentityV1 {
    fn key(self) -> (u8, String) {
        match self {
            Self::Boundary { case } => (0, case.to_wire()),
            Self::InputValue { case } => (1, case.to_wire()),
            Self::MemorySurface { case } => (2, case.to_wire()),
        }
    }

    fn matches_invocation(self, invocation: CorpusInvocationIdentityV1) -> bool {
        matches!(
            (self, invocation),
            (
                Self::Boundary { .. },
                CorpusInvocationIdentityV1::Boundary { .. }
            ) | (
                Self::InputValue { .. },
                CorpusInvocationIdentityV1::InputValue { .. }
            ) | (
                Self::MemorySurface { .. },
                CorpusInvocationIdentityV1::MemorySurface { .. }
            )
        )
    }
}

/// One independently executable generic job in a complete corpus plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusExecutionPlanItemV1 {
    obligation: CorpusObligationIdentityV1,
    invocation: CorpusInvocationIdentityV1,
    expected_outcome: CaseExpectedOutcome,
    request: ContentId<CallAdapterRequestArtifact>,
    input_bundle: ContentId<InputBundleArtifact>,
    job_id: JobId,
    contract: ContentId<JobContractArtifact>,
}

impl CorpusExecutionPlanItemV1 {
    #[must_use]
    pub const fn obligation(&self) -> CorpusObligationIdentityV1 {
        self.obligation
    }

    #[must_use]
    pub const fn invocation(&self) -> CorpusInvocationIdentityV1 {
        self.invocation
    }

    #[must_use]
    pub const fn expected_outcome(&self) -> &CaseExpectedOutcome {
        &self.expected_outcome
    }

    #[must_use]
    pub const fn request(&self) -> ContentId<CallAdapterRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn input_bundle(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle
    }

    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<JobContractArtifact> {
        self.contract
    }
}

/// Strict V1 immutable plan covering every executable mandatory corpus obligation exactly once.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CorpusExecutionPlanWire")]
pub struct CorpusExecutionPlanV1 {
    schema_version: u16,
    domain: ContentId<CallerDomainBodyArtifact>,
    quantitative_obligations: ContentId<MigrationMandatoryCasesArtifact>,
    input_value_obligations: ContentId<MandatoryInputValueCasesArtifact>,
    memory_surface_obligations: ContentId<MandatoryMemorySurfaceCasesArtifact>,
    subject: CorpusExecutionSubjectV1,
    executable: ContentId<CallAdapterExecutableArtifact>,
    tier: MigrationValidationTier,
    items: Vec<CorpusExecutionPlanItemV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusExecutionPlanWire {
    schema_version: u16,
    domain: ContentId<CallerDomainBodyArtifact>,
    quantitative_obligations: ContentId<MigrationMandatoryCasesArtifact>,
    input_value_obligations: ContentId<MandatoryInputValueCasesArtifact>,
    memory_surface_obligations: ContentId<MandatoryMemorySurfaceCasesArtifact>,
    subject: CorpusExecutionSubjectV1,
    executable: ContentId<CallAdapterExecutableArtifact>,
    tier: MigrationValidationTier,
    items: Vec<CorpusExecutionPlanItemV1>,
}

impl CorpusExecutionPlanV1 {
    #[expect(
        clippy::too_many_arguments,
        reason = "domain, three obligation roots, subject, executable, tier, and items are independent immutable bindings"
    )]
    fn new(
        domain: ContentId<CallerDomainBodyArtifact>,
        quantitative_obligations: ContentId<MigrationMandatoryCasesArtifact>,
        input_value_obligations: ContentId<MandatoryInputValueCasesArtifact>,
        memory_surface_obligations: ContentId<MandatoryMemorySurfaceCasesArtifact>,
        subject: CorpusExecutionSubjectV1,
        executable: ContentId<CallAdapterExecutableArtifact>,
        tier: MigrationValidationTier,
        items: Vec<CorpusExecutionPlanItemV1>,
    ) -> Result<Self, CorpusExecutionPlanError> {
        if items.is_empty()
            || items
                .windows(2)
                .any(|pair| pair[0].obligation.key() >= pair[1].obligation.key())
            || items.iter().any(|item| {
                !item.obligation.matches_invocation(item.invocation)
                    || matches!(
                        item.expected_outcome,
                        CaseExpectedOutcome::Invalid {
                            behavior: InvalidInputBehavior::ExplicitlyExcluded
                        }
                    )
            })
        {
            return Err(CorpusExecutionPlanError::InconsistentPlan);
        }
        let mut jobs = BTreeSet::new();
        let mut invocations = BTreeSet::new();
        let mut requests = BTreeSet::new();
        let mut bundles = BTreeSet::new();
        let mut contracts = BTreeSet::new();
        if items.iter().any(|item| {
            !jobs.insert(item.job_id.to_string())
                || !invocations.insert(invocation_key(item.invocation))
                || !requests.insert(item.request.to_wire())
                || !bundles.insert(item.input_bundle.to_wire())
                || !contracts.insert(item.contract.to_wire())
        }) {
            return Err(CorpusExecutionPlanError::InconsistentPlan);
        }
        Ok(Self {
            schema_version: 1,
            domain,
            quantitative_obligations,
            input_value_obligations,
            memory_surface_obligations,
            subject,
            executable,
            tier,
            items,
        })
    }

    #[must_use]
    pub const fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.domain
    }

    #[must_use]
    pub const fn quantitative_obligations(&self) -> ContentId<MigrationMandatoryCasesArtifact> {
        self.quantitative_obligations
    }

    #[must_use]
    pub const fn input_value_obligations(&self) -> ContentId<MandatoryInputValueCasesArtifact> {
        self.input_value_obligations
    }

    #[must_use]
    pub const fn memory_surface_obligations(
        &self,
    ) -> ContentId<MandatoryMemorySurfaceCasesArtifact> {
        self.memory_surface_obligations
    }

    #[must_use]
    pub const fn subject(&self) -> CorpusExecutionSubjectV1 {
        self.subject
    }

    #[must_use]
    pub const fn executable(&self) -> ContentId<CallAdapterExecutableArtifact> {
        self.executable
    }

    #[must_use]
    pub const fn tier(&self) -> MigrationValidationTier {
        self.tier
    }

    #[must_use]
    pub fn items(&self) -> &[CorpusExecutionPlanItemV1] {
        &self.items
    }

    /// Recomputes the three mandatory roots and exact executable subset.
    ///
    /// # Errors
    ///
    /// Rejects different obligation sets, changed expectation metadata, missing cases, or cases
    /// that are unknown or explicitly excluded.
    pub fn validate_obligations(
        &self,
        quantitative: &MigrationMandatoryCasesV1,
        input_values: &MandatoryInputValueCasesV1,
        memory_surfaces: &MandatoryMemorySurfaceCasesV1,
    ) -> Result<(), CorpusExecutionPlanError> {
        let roots = obligation_roots(quantitative, input_values, memory_surfaces)?;
        if self.domain != roots.domain {
            return Err(CorpusExecutionPlanError::ObligationDomainMismatch);
        }
        if self.quantitative_obligations != roots.quantitative
            || self.input_value_obligations != roots.input_values
            || self.memory_surface_obligations != roots.memory_surfaces
        {
            return Err(CorpusExecutionPlanError::InconsistentPlan);
        }
        let required = required_obligations(quantitative, input_values, memory_surfaces)?;
        if self.items.len() != required.len()
            || self.items.iter().zip(required).any(|(item, required)| {
                item.obligation != required.obligation
                    || item.expected_outcome != required.expected_outcome
            })
        {
            return Err(CorpusExecutionPlanError::InconsistentPlan);
        }
        Ok(())
    }
}

impl TryFrom<CorpusExecutionPlanWire> for CorpusExecutionPlanV1 {
    type Error = CorpusExecutionPlanError;

    fn try_from(wire: CorpusExecutionPlanWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(CorpusExecutionPlanError::UnsupportedSchemaVersion);
        }
        Self::new(
            wire.domain,
            wire.quantitative_obligations,
            wire.input_value_obligations,
            wire.memory_surface_obligations,
            wire.subject,
            wire.executable,
            wire.tier,
            wire.items,
        )
    }
}

/// Content domain for one exact immutable executable-corpus plan.
pub enum CorpusExecutionPlanArtifact {}

impl ContentType for CorpusExecutionPlanArtifact {
    const DOMAIN: &'static str = "migration.corpus-execution-plan.v1";
}

/// One assembled case plus its caller-assigned independent execution lifecycle identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssembledCorpusExecutionCase {
    Boundary {
        job_id: JobId,
        case: AssembledBoundaryCaseInput,
    },
    InputValue {
        job_id: JobId,
        case: AssembledInputValueCaseInput,
    },
    MemorySurface {
        job_id: JobId,
        case: AssembledMemorySurfaceCaseInput,
    },
}

impl AssembledCorpusExecutionCase {
    fn job_id(&self) -> JobId {
        match self {
            Self::Boundary { job_id, .. }
            | Self::InputValue { job_id, .. }
            | Self::MemorySurface { job_id, .. } => *job_id,
        }
    }

    fn obligation(&self) -> CorpusObligationIdentityV1 {
        match self {
            Self::Boundary { case, .. } => CorpusObligationIdentityV1::Boundary {
                case: case.manifest().boundary_case(),
            },
            Self::InputValue { case, .. } => CorpusObligationIdentityV1::InputValue {
                case: case.manifest().input_value_case(),
            },
            Self::MemorySurface { case, .. } => CorpusObligationIdentityV1::MemorySurface {
                case: case.manifest().memory_surface_case(),
            },
        }
    }

    fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        match self {
            Self::Boundary { case, .. } => case.manifest().domain(),
            Self::InputValue { case, .. } => case.manifest().domain(),
            Self::MemorySurface { case, .. } => case.manifest().domain(),
        }
    }

    fn expected_outcome(&self) -> &CaseExpectedOutcome {
        match self {
            Self::Boundary { case, .. } => case.manifest().expected_outcome(),
            Self::InputValue { case, .. } => case.manifest().expected_outcome(),
            Self::MemorySurface { case, .. } => case.manifest().expected_outcome(),
        }
    }
}

/// Prepared item retaining case-specific types needed for later receipt validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedCorpusExecutionCase {
    Boundary {
        descriptor: CorpusExecutionPlanItemV1,
        case: AssembledBoundaryCaseInput,
        input: PreparedCallAdapterInput,
        job: PreparedCallAdapterJob,
    },
    InputValue {
        descriptor: CorpusExecutionPlanItemV1,
        case: AssembledInputValueCaseInput,
        input: PreparedCallAdapterInput,
        job: PreparedCallAdapterJob,
    },
    MemorySurface {
        descriptor: CorpusExecutionPlanItemV1,
        case: AssembledMemorySurfaceCaseInput,
        input: PreparedCallAdapterInput,
        job: PreparedCallAdapterJob,
    },
}

impl PreparedCorpusExecutionCase {
    #[must_use]
    pub const fn descriptor(&self) -> &CorpusExecutionPlanItemV1 {
        match self {
            Self::Boundary { descriptor, .. }
            | Self::InputValue { descriptor, .. }
            | Self::MemorySurface { descriptor, .. } => descriptor,
        }
    }

    #[must_use]
    pub const fn input(&self) -> &PreparedCallAdapterInput {
        match self {
            Self::Boundary { input, .. }
            | Self::InputValue { input, .. }
            | Self::MemorySurface { input, .. } => input,
        }
    }

    #[must_use]
    pub const fn job(&self) -> &PreparedCallAdapterJob {
        match self {
            Self::Boundary { job, .. }
            | Self::InputValue { job, .. }
            | Self::MemorySurface { job, .. } => job,
        }
    }
}

/// Canonical plan and all transient prepared materials required to execute it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCorpusExecutionPlan {
    plan: CorpusExecutionPlanV1,
    plan_bytes: Vec<u8>,
    plan_id: ContentId<CorpusExecutionPlanArtifact>,
    cases: Vec<PreparedCorpusExecutionCase>,
}

impl PreparedCorpusExecutionPlan {
    #[must_use]
    pub const fn plan(&self) -> &CorpusExecutionPlanV1 {
        &self.plan
    }

    #[must_use]
    pub fn plan_bytes(&self) -> &[u8] {
        &self.plan_bytes
    }

    #[must_use]
    pub const fn plan_id(&self) -> ContentId<CorpusExecutionPlanArtifact> {
        self.plan_id
    }

    #[must_use]
    pub fn cases(&self) -> &[PreparedCorpusExecutionCase] {
        &self.cases
    }
}

/// Builds one canonical plan covering every executable mandatory obligation exactly once.
///
/// Unknown and explicitly excluded obligations remain committed by their mandatory-set roots but
/// do not become jobs. The subject binds the executable run to its source/reference/variant/
/// candidate role. Each executable case receives its own caller-supplied `JobId` and therefore its
/// own generic execution lifecycle.
///
/// # Errors
///
/// Rejects different mandatory domains, missing/duplicate/extra assembled cases, case expectation
/// contradictions, adapter preparation failures, or non-canonical plan material.
#[expect(
    clippy::too_many_arguments,
    reason = "three obligation roots, assembled cases, executable, environment, need, and limits are independent trust inputs"
)]
pub fn prepare_corpus_execution_plan(
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
    subject: CorpusExecutionSubjectV1,
    cases: Vec<AssembledCorpusExecutionCase>,
    executable: &[u8],
    executable_limit: CallAdapterExecutableByteLimit,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: CallAdapterCaptureLimits,
) -> Result<PreparedCorpusExecutionPlan, CorpusExecutionPlanError> {
    let roots = obligation_roots(quantitative, input_values, memory_surfaces)?;
    let required = required_obligations(quantitative, input_values, memory_surfaces)?;
    let mut supplied = BTreeMap::new();
    for case in cases {
        let obligation = case.obligation();
        if supplied.insert(obligation.key(), case).is_some() {
            return Err(CorpusExecutionPlanError::DuplicateCase { obligation });
        }
    }
    let mut ordered = Vec::with_capacity(required.len());
    for requirement in &required {
        let case = supplied.remove(&requirement.obligation.key()).ok_or(
            CorpusExecutionPlanError::MissingCase {
                obligation: requirement.obligation,
            },
        )?;
        if case.domain() != roots.domain || case.expected_outcome() != &requirement.expected_outcome
        {
            return Err(CorpusExecutionPlanError::InconsistentCase);
        }
        ordered.push(case);
    }
    if let Some(case) = supplied.into_values().next() {
        return Err(CorpusExecutionPlanError::UnexpectedCase {
            obligation: case.obligation(),
        });
    }

    let mut prepared = Vec::with_capacity(ordered.len());
    for case in ordered {
        prepared.push(prepare_case(
            case,
            executable,
            executable_limit,
            environment,
            need,
            limits,
        )?);
    }
    let executable_id =
        ContentId::<CallAdapterExecutableArtifact>::derive(executable).map_err(plan_codec)?;
    let descriptors = prepared
        .iter()
        .map(|case| case.descriptor().clone())
        .collect();
    let plan = CorpusExecutionPlanV1::new(
        roots.domain,
        roots.quantitative,
        roots.input_values,
        roots.memory_surfaces,
        subject,
        executable_id,
        need.tier(),
        descriptors,
    )?;
    plan.validate_obligations(quantitative, input_values, memory_surfaces)?;
    let plan_bytes = cairn_codec::to_vec(&plan).map_err(plan_codec)?;
    let plan_id =
        ContentId::<CorpusExecutionPlanArtifact>::derive(&plan_bytes).map_err(plan_codec)?;
    Ok(PreparedCorpusExecutionPlan {
        plan,
        plan_bytes,
        plan_id,
        cases: prepared,
    })
}

fn prepare_case(
    case: AssembledCorpusExecutionCase,
    executable: &[u8],
    executable_limit: CallAdapterExecutableByteLimit,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: CallAdapterCaptureLimits,
) -> Result<PreparedCorpusExecutionCase, CorpusExecutionPlanError> {
    let job_id = case.job_id();
    let obligation = case.obligation();
    let expected_outcome = case.expected_outcome().clone();
    match case {
        AssembledCorpusExecutionCase::Boundary { case, .. } => {
            let input = prepare_boundary_call_adapter_input(&case, executable, executable_limit)
                .map_err(adapter_error)?;
            let job = compose_call_adapter_job(job_id, &input, environment, need, limits)
                .map_err(adapter_error)?;
            let descriptor = descriptor(obligation, expected_outcome, &input, &job);
            Ok(PreparedCorpusExecutionCase::Boundary {
                descriptor,
                case,
                input,
                job,
            })
        }
        AssembledCorpusExecutionCase::InputValue { case, .. } => {
            let input = prepare_input_value_call_adapter_input(&case, executable, executable_limit)
                .map_err(adapter_error)?;
            let job = compose_call_adapter_job(job_id, &input, environment, need, limits)
                .map_err(adapter_error)?;
            let descriptor = descriptor(obligation, expected_outcome, &input, &job);
            Ok(PreparedCorpusExecutionCase::InputValue {
                descriptor,
                case,
                input,
                job,
            })
        }
        AssembledCorpusExecutionCase::MemorySurface { case, .. } => {
            let input =
                prepare_memory_surface_call_adapter_input(&case, executable, executable_limit)
                    .map_err(adapter_error)?;
            let job = compose_call_adapter_job(job_id, &input, environment, need, limits)
                .map_err(adapter_error)?;
            let descriptor = descriptor(obligation, expected_outcome, &input, &job);
            Ok(PreparedCorpusExecutionCase::MemorySurface {
                descriptor,
                case,
                input,
                job,
            })
        }
    }
}

fn descriptor(
    obligation: CorpusObligationIdentityV1,
    expected_outcome: CaseExpectedOutcome,
    input: &PreparedCallAdapterInput,
    job: &PreparedCallAdapterJob,
) -> CorpusExecutionPlanItemV1 {
    CorpusExecutionPlanItemV1 {
        obligation,
        invocation: input.request().invocation(),
        expected_outcome,
        request: input.request_id(),
        input_bundle: input.input_bundle_id(),
        job_id: job.contract().job_id(),
        contract: job.contract_id(),
    }
}

#[derive(Clone, Copy)]
struct ObligationRoots {
    domain: ContentId<CallerDomainBodyArtifact>,
    quantitative: ContentId<MigrationMandatoryCasesArtifact>,
    input_values: ContentId<MandatoryInputValueCasesArtifact>,
    memory_surfaces: ContentId<MandatoryMemorySurfaceCasesArtifact>,
}

fn obligation_roots(
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
) -> Result<ObligationRoots, CorpusExecutionPlanError> {
    if quantitative.domain() != input_values.domain()
        || quantitative.domain() != memory_surfaces.domain()
    {
        return Err(CorpusExecutionPlanError::ObligationDomainMismatch);
    }
    Ok(ObligationRoots {
        domain: quantitative.domain(),
        quantitative: canonical_id(quantitative)?,
        input_values: canonical_id(input_values)?,
        memory_surfaces: canonical_id(memory_surfaces)?,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequiredObligation {
    obligation: CorpusObligationIdentityV1,
    expected_outcome: CaseExpectedOutcome,
}

fn required_obligations(
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
) -> Result<Vec<RequiredObligation>, CorpusExecutionPlanError> {
    let mut required = Vec::new();
    for case in quantitative.cases() {
        if executable_outcome(case.expected_outcome()).is_some() {
            required.push(RequiredObligation {
                obligation: CorpusObligationIdentityV1::Boundary {
                    case: canonical_id(case)?,
                },
                expected_outcome: case.expected_outcome().clone(),
            });
        }
    }
    for case in input_values.cases() {
        if let Some(expected_outcome) = input_value_outcome(case.disposition()) {
            required.push(RequiredObligation {
                obligation: CorpusObligationIdentityV1::InputValue {
                    case: canonical_id(case)?,
                },
                expected_outcome,
            });
        }
    }
    for case in memory_surfaces.cases() {
        if let Some(expected_outcome) = memory_surface_outcome(case.disposition()) {
            required.push(RequiredObligation {
                obligation: CorpusObligationIdentityV1::MemorySurface {
                    case: canonical_id(case)?,
                },
                expected_outcome,
            });
        }
    }
    required.sort_by_key(|value| value.obligation.key());
    Ok(required)
}

fn executable_outcome(outcome: &CaseExpectedOutcome) -> Option<CaseExpectedOutcome> {
    match outcome {
        CaseExpectedOutcome::Success => Some(CaseExpectedOutcome::Success),
        CaseExpectedOutcome::Invalid { behavior }
            if behavior != &InvalidInputBehavior::ExplicitlyExcluded =>
        {
            Some(outcome.clone())
        }
        CaseExpectedOutcome::Invalid { .. } => None,
    }
}

fn input_value_outcome(disposition: &InputValueDisposition) -> Option<CaseExpectedOutcome> {
    match disposition {
        InputValueDisposition::Supported => Some(CaseExpectedOutcome::Success),
        InputValueDisposition::Invalid { behavior }
            if behavior != &InvalidInputBehavior::ExplicitlyExcluded =>
        {
            Some(CaseExpectedOutcome::Invalid {
                behavior: behavior.clone(),
            })
        }
        InputValueDisposition::Invalid { .. }
        | InputValueDisposition::ExplicitlyExcluded { .. }
        | InputValueDisposition::Unknown => None,
    }
}

fn memory_surface_outcome(disposition: &MemoryConditionDisposition) -> Option<CaseExpectedOutcome> {
    match disposition {
        MemoryConditionDisposition::Supported => Some(CaseExpectedOutcome::Success),
        MemoryConditionDisposition::Invalid { behavior }
            if behavior != &InvalidInputBehavior::ExplicitlyExcluded =>
        {
            Some(CaseExpectedOutcome::Invalid {
                behavior: behavior.clone(),
            })
        }
        MemoryConditionDisposition::Invalid { .. }
        | MemoryConditionDisposition::ExplicitlyExcluded { .. }
        | MemoryConditionDisposition::Unknown => None,
    }
}

fn invocation_key(invocation: CorpusInvocationIdentityV1) -> (u8, String) {
    match invocation {
        CorpusInvocationIdentityV1::Boundary { manifest } => (0, manifest.to_wire()),
        CorpusInvocationIdentityV1::InputValue { manifest } => (1, manifest.to_wire()),
        CorpusInvocationIdentityV1::MemorySurface { manifest } => (2, manifest.to_wire()),
    }
}

fn canonical_id<T: ContentType, V: Serialize>(
    value: &V,
) -> Result<ContentId<T>, CorpusExecutionPlanError> {
    let bytes = cairn_codec::to_vec(value).map_err(plan_codec)?;
    ContentId::<T>::derive(&bytes).map_err(plan_codec)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the helper is passed directly to Result::map_err, which supplies an owned protocol error"
)]
fn adapter_error(error: CallAdapterProtocolError) -> CorpusExecutionPlanError {
    CorpusExecutionPlanError::Adapter {
        message: error.to_string(),
    }
}

fn plan_codec(error: impl std::fmt::Display) -> CorpusExecutionPlanError {
    CorpusExecutionPlanError::Codec {
        message: error.to_string(),
    }
}
