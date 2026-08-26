//! Exact collection of complete-corpus execution receipts into typed observations.

use std::collections::{BTreeMap, BTreeSet};

use cairn_execution::{ExecutionReceipt, ExecutionReceiptArtifact};
use cairn_protocol::{ContentId, ContentType, JobId};
use cairn_record::ContentStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CallAdapterProtocolError, CallAdapterResultArtifact, CorpusExecutionPlanArtifact,
    CorpusExecutionPlanV1, CorpusObligationIdentityV1, PreparedCorpusExecutionCase,
    PreparedCorpusExecutionPlan, ValidatedCallAdapterExecution,
    validate_boundary_call_adapter_receipt, validate_input_value_call_adapter_receipt,
    validate_memory_surface_call_adapter_receipt,
};

/// Failure to collect one exact authoritative receipt for every planned corpus job.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CorpusObservationSetError {
    /// Only the current pre-release V1 observation set is accepted.
    #[error("corpus observation set schema version must be 1")]
    UnsupportedSchemaVersion,
    /// The same planned job was supplied more than once.
    #[error("duplicate corpus receipt for job {job_id}")]
    DuplicateReceipt { job_id: JobId },
    /// A planned job has no authoritative receipt.
    #[error("missing corpus receipt for job {job_id}")]
    MissingReceipt { job_id: JobId },
    /// A receipt belongs to no job in the exact plan.
    #[error("unexpected corpus receipt for job {job_id}")]
    UnexpectedReceipt { job_id: JobId },
    /// Transient prepared cases no longer match their immutable plan descriptors.
    #[error("prepared corpus cases contradict their execution plan")]
    InconsistentPreparedPlan,
    /// Persisted observations are non-canonical or contradict their cited plan.
    #[error("corpus observation set is inconsistent")]
    InconsistentObservationSet,
    /// One receipt or its declared adapter outputs failed exact typed validation.
    #[error("corpus receipt validation failed for job {job_id}: {message}")]
    Receipt { job_id: JobId, message: String },
    /// Canonical encoding or typed identity derivation failed.
    #[error("corpus observation set codec error: {message}")]
    Codec { message: String },
}

/// One authoritative generic receipt supplied for complete-corpus collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusExecutionReceipt {
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
}

impl CorpusExecutionReceipt {
    #[must_use]
    pub const fn new(
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        receipt: ExecutionReceipt,
    ) -> Self {
        Self {
            receipt_id,
            receipt,
        }
    }

    #[must_use]
    pub const fn receipt_id(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt_id
    }

    #[must_use]
    pub const fn receipt(&self) -> &ExecutionReceipt {
        &self.receipt
    }
}

/// Persisted observation binding for one exact plan item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusObservationItemV1 {
    obligation: CorpusObligationIdentityV1,
    job_id: JobId,
    receipt: ContentId<ExecutionReceiptArtifact>,
    result: ContentId<CallAdapterResultArtifact>,
}

impl CorpusObservationItemV1 {
    #[must_use]
    pub const fn obligation(&self) -> CorpusObligationIdentityV1 {
        self.obligation
    }

    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    #[must_use]
    pub const fn receipt(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt
    }

    #[must_use]
    pub const fn result(&self) -> ContentId<CallAdapterResultArtifact> {
        self.result
    }
}

/// Strict V1 record proving that every job in one exact corpus plan produced a validated
/// adapter observation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CorpusObservationSetWire")]
pub struct CorpusObservationSetV1 {
    schema_version: u16,
    plan: ContentId<CorpusExecutionPlanArtifact>,
    observations: Vec<CorpusObservationItemV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusObservationSetWire {
    schema_version: u16,
    plan: ContentId<CorpusExecutionPlanArtifact>,
    observations: Vec<CorpusObservationItemV1>,
}

impl CorpusObservationSetV1 {
    fn new(
        plan: ContentId<CorpusExecutionPlanArtifact>,
        observations: Vec<CorpusObservationItemV1>,
    ) -> Result<Self, CorpusObservationSetError> {
        if observations.is_empty()
            || observations.windows(2).any(|pair| {
                observation_key(pair[0].obligation) >= observation_key(pair[1].obligation)
            })
        {
            return Err(CorpusObservationSetError::InconsistentObservationSet);
        }
        let mut jobs = BTreeSet::new();
        let mut receipts = BTreeSet::new();
        let mut results = BTreeSet::new();
        if observations.iter().any(|observation| {
            !jobs.insert(observation.job_id.to_string())
                || !receipts.insert(observation.receipt.to_wire())
                || !results.insert(observation.result.to_wire())
        }) {
            return Err(CorpusObservationSetError::InconsistentObservationSet);
        }
        Ok(Self {
            schema_version: 1,
            plan,
            observations,
        })
    }

    #[must_use]
    pub const fn plan(&self) -> ContentId<CorpusExecutionPlanArtifact> {
        self.plan
    }

    #[must_use]
    pub fn observations(&self) -> &[CorpusObservationItemV1] {
        &self.observations
    }

    /// Recomputes the plan identity and requires one exact observation per plan item.
    ///
    /// # Errors
    ///
    /// Rejects a changed plan, missing/reordered observations, or job/obligation mismatches.
    pub fn validate_plan(
        &self,
        plan: &CorpusExecutionPlanV1,
    ) -> Result<(), CorpusObservationSetError> {
        let plan_bytes = cairn_codec::to_vec(plan).map_err(codec)?;
        if ContentId::<CorpusExecutionPlanArtifact>::derive(&plan_bytes).map_err(codec)?
            != self.plan
            || self.observations.len() != plan.items().len()
            || self
                .observations
                .iter()
                .zip(plan.items())
                .any(|(observation, item)| {
                    observation.obligation != item.obligation()
                        || observation.job_id != item.job_id()
                })
        {
            return Err(CorpusObservationSetError::InconsistentObservationSet);
        }
        Ok(())
    }
}

impl TryFrom<CorpusObservationSetWire> for CorpusObservationSetV1 {
    type Error = CorpusObservationSetError;

    fn try_from(wire: CorpusObservationSetWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(CorpusObservationSetError::UnsupportedSchemaVersion);
        }
        Self::new(wire.plan, wire.observations)
    }
}

/// Content domain for one exact complete-corpus observation set.
pub enum CorpusObservationSetArtifact {}

impl ContentType for CorpusObservationSetArtifact {
    const DOMAIN: &'static str = "migration.corpus-observation-set.v1";
}

/// One validated execution retaining its obligation category at the Rust boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidatedCorpusExecutionCase {
    Boundary {
        observation: CorpusObservationItemV1,
        execution: ValidatedCallAdapterExecution,
    },
    InputValue {
        observation: CorpusObservationItemV1,
        execution: ValidatedCallAdapterExecution,
    },
    MemorySurface {
        observation: CorpusObservationItemV1,
        execution: ValidatedCallAdapterExecution,
    },
}

impl ValidatedCorpusExecutionCase {
    #[must_use]
    pub const fn observation(&self) -> &CorpusObservationItemV1 {
        match self {
            Self::Boundary { observation, .. }
            | Self::InputValue { observation, .. }
            | Self::MemorySurface { observation, .. } => observation,
        }
    }

    #[must_use]
    pub const fn execution(&self) -> &ValidatedCallAdapterExecution {
        match self {
            Self::Boundary { execution, .. }
            | Self::InputValue { execution, .. }
            | Self::MemorySurface { execution, .. } => execution,
        }
    }
}

/// Canonical persisted set plus category-preserving validated executions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCorpusObservationSet {
    observation_set: CorpusObservationSetV1,
    observation_set_bytes: Vec<u8>,
    observation_set_id: ContentId<CorpusObservationSetArtifact>,
    cases: Vec<ValidatedCorpusExecutionCase>,
}

impl ValidatedCorpusObservationSet {
    #[must_use]
    pub const fn observation_set(&self) -> &CorpusObservationSetV1 {
        &self.observation_set
    }

    #[must_use]
    pub fn observation_set_bytes(&self) -> &[u8] {
        &self.observation_set_bytes
    }

    #[must_use]
    pub const fn observation_set_id(&self) -> ContentId<CorpusObservationSetArtifact> {
        self.observation_set_id
    }

    #[must_use]
    pub fn cases(&self) -> &[ValidatedCorpusExecutionCase] {
        &self.cases
    }
}

/// Validates and collects exactly one authoritative receipt per complete-plan job.
///
/// This establishes execution and adapter-observation completeness only. It does not compare
/// outputs, admit an oracle, or issue a semantic verdict.
///
/// # Errors
///
/// Rejects inconsistent prepared material, missing/duplicate/extra job receipts, receipt identity
/// or contract mismatches, missing/changed declared outputs, and non-canonical persisted material.
pub fn validate_corpus_execution_receipts<C: ContentStore>(
    plan: &PreparedCorpusExecutionPlan,
    receipts: Vec<CorpusExecutionReceipt>,
    content: &C,
) -> Result<ValidatedCorpusObservationSet, CorpusObservationSetError> {
    validate_prepared_plan(plan)?;
    let expected_jobs = plan
        .plan()
        .items()
        .iter()
        .map(|item| item.job_id().to_string())
        .collect::<BTreeSet<_>>();
    let mut supplied = BTreeMap::new();
    for receipt in receipts {
        let job_id = receipt.receipt.job_id();
        if supplied.insert(job_id.to_string(), receipt).is_some() {
            return Err(CorpusObservationSetError::DuplicateReceipt { job_id });
        }
    }
    if let Some(receipt) = supplied
        .iter()
        .find(|(job, _)| !expected_jobs.contains(*job))
        .map(|(_, receipt)| receipt)
    {
        return Err(CorpusObservationSetError::UnexpectedReceipt {
            job_id: receipt.receipt.job_id(),
        });
    }

    let mut cases = Vec::with_capacity(plan.cases().len());
    for prepared in plan.cases() {
        let job_id = prepared.descriptor().job_id();
        let receipt = supplied
            .remove(&job_id.to_string())
            .ok_or(CorpusObservationSetError::MissingReceipt { job_id })?;
        cases.push(validate_case(prepared, &receipt, content)?);
    }
    if !supplied.is_empty() {
        return Err(CorpusObservationSetError::InconsistentObservationSet);
    }
    let observations = cases
        .iter()
        .map(|case| case.observation().clone())
        .collect();
    let observation_set = CorpusObservationSetV1::new(plan.plan_id(), observations)?;
    observation_set.validate_plan(plan.plan())?;
    let observation_set_bytes = cairn_codec::to_vec(&observation_set).map_err(codec)?;
    let observation_set_id =
        ContentId::<CorpusObservationSetArtifact>::derive(&observation_set_bytes).map_err(codec)?;
    Ok(ValidatedCorpusObservationSet {
        observation_set,
        observation_set_bytes,
        observation_set_id,
        cases,
    })
}

fn validate_prepared_plan(
    plan: &PreparedCorpusExecutionPlan,
) -> Result<(), CorpusObservationSetError> {
    if plan.cases().len() != plan.plan().items().len()
        || plan
            .cases()
            .iter()
            .zip(plan.plan().items())
            .any(|(case, item)| case.descriptor() != item)
    {
        return Err(CorpusObservationSetError::InconsistentPreparedPlan);
    }
    Ok(())
}

fn validate_case<C: ContentStore>(
    prepared: &PreparedCorpusExecutionCase,
    receipt: &CorpusExecutionReceipt,
    content: &C,
) -> Result<ValidatedCorpusExecutionCase, CorpusObservationSetError> {
    let job_id = prepared.descriptor().job_id();
    let receipt_id = receipt.receipt_id;
    let execution = match prepared {
        PreparedCorpusExecutionCase::Boundary {
            case, input, job, ..
        } => validate_boundary_call_adapter_receipt(
            case,
            input,
            job,
            receipt_id,
            &receipt.receipt,
            content,
        ),
        PreparedCorpusExecutionCase::InputValue {
            case, input, job, ..
        } => validate_input_value_call_adapter_receipt(
            case,
            input,
            job,
            receipt_id,
            &receipt.receipt,
            content,
        ),
        PreparedCorpusExecutionCase::MemorySurface {
            case, input, job, ..
        } => validate_memory_surface_call_adapter_receipt(
            case,
            input,
            job,
            receipt_id,
            &receipt.receipt,
            content,
        ),
    }
    .map_err(|error| receipt_error(job_id, &error))?;
    let observation = CorpusObservationItemV1 {
        obligation: prepared.descriptor().obligation(),
        job_id,
        receipt: execution.receipt_id(),
        result: execution.observation().result_id(),
    };
    Ok(match prepared {
        PreparedCorpusExecutionCase::Boundary { .. } => ValidatedCorpusExecutionCase::Boundary {
            observation,
            execution,
        },
        PreparedCorpusExecutionCase::InputValue { .. } => {
            ValidatedCorpusExecutionCase::InputValue {
                observation,
                execution,
            }
        }
        PreparedCorpusExecutionCase::MemorySurface { .. } => {
            ValidatedCorpusExecutionCase::MemorySurface {
                observation,
                execution,
            }
        }
    })
}

fn observation_key(obligation: CorpusObligationIdentityV1) -> (u8, String) {
    match obligation {
        CorpusObligationIdentityV1::Boundary { case } => (0, case.to_wire()),
        CorpusObligationIdentityV1::InputValue { case } => (1, case.to_wire()),
        CorpusObligationIdentityV1::MemorySurface { case } => (2, case.to_wire()),
    }
}

fn receipt_error(job_id: JobId, error: &CallAdapterProtocolError) -> CorpusObservationSetError {
    CorpusObservationSetError::Receipt {
        job_id,
        message: error.to_string(),
    }
}

fn codec(error: impl std::fmt::Display) -> CorpusObservationSetError {
    CorpusObservationSetError::Codec {
        message: error.to_string(),
    }
}
