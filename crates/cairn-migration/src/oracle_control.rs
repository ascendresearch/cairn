//! Controller-owned execution boundary for qualified Oracle Admission controls.
#![allow(clippy::missing_errors_doc)]

use cairn_execution::{ExecutionReceipt, ExecutionReceiptArtifact, JobContractArtifact};
use cairn_protocol::{AttemptId, ContentId, ContentType, JobId};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    OracleAdmissionAttemptArtifact, OracleAdmissionAttemptV1, OracleAdmissionMechanismCatalogV1,
    OracleControlFamilyV1, OracleControlObligationV1, OracleControlResultV1, OracleFrameworkError,
    OracleQualifiedMechanismArtifact, TrustedOracleControlReceiptArtifact,
};

const SCHEMA_V1: u16 = 1;

macro_rules! artifact {
    ($name:ident, $domain:literal) => {
        pub enum $name {}
        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

artifact!(
    OracleControlRunnerArtifact,
    "migration.oracle-control-runner.v1"
);
artifact!(
    OracleMechanismQualificationReceiptArtifact,
    "migration.oracle-mechanism-qualification-receipt.v1"
);
artifact!(OracleControlRunArtifact, "migration.oracle-control-run.v1");
artifact!(
    OracleControlDispatchArtifact,
    "migration.oracle-control-dispatch.v1"
);

/// Canonical prior qualification binding one control mechanism to its executable runner evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleMechanismQualificationReceiptV1 {
    schema_version: u16,
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
    runner: ContentId<OracleControlRunnerArtifact>,
    evidence: ContentId<ExecutionReceiptArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleMechanismQualificationReceiptWire {
    schema_version: u16,
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
    runner: ContentId<OracleControlRunnerArtifact>,
    evidence: ContentId<ExecutionReceiptArtifact>,
}

impl OracleMechanismQualificationReceiptV1 {
    #[must_use]
    pub const fn new(
        control: OracleControlFamilyV1,
        mechanism: ContentId<OracleQualifiedMechanismArtifact>,
        runner: ContentId<OracleControlRunnerArtifact>,
        evidence: ContentId<ExecutionReceiptArtifact>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            control,
            mechanism,
            runner,
            evidence,
        }
    }

    #[must_use]
    pub const fn control(&self) -> OracleControlFamilyV1 {
        self.control
    }

    #[must_use]
    pub const fn mechanism(&self) -> ContentId<OracleQualifiedMechanismArtifact> {
        self.mechanism
    }

    #[must_use]
    pub const fn runner(&self) -> ContentId<OracleControlRunnerArtifact> {
        self.runner
    }

    #[must_use]
    pub const fn evidence(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.evidence
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleMechanismQualificationReceiptArtifact>, OracleControlError> {
        self.validate()?;
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleControlError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleControlError::UnsupportedSchema);
        }
        Ok(())
    }
}

impl TryFrom<OracleMechanismQualificationReceiptWire> for OracleMechanismQualificationReceiptV1 {
    type Error = OracleControlError;

    fn try_from(wire: OracleMechanismQualificationReceiptWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            control: wire.control,
            mechanism: wire.mechanism,
            runner: wire.runner,
            evidence: wire.evidence,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleMechanismQualificationReceiptV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        OracleMechanismQualificationReceiptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One exact mechanically required control bound to its qualified runner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleControlRunV1 {
    schema_version: u16,
    attempt: ContentId<OracleAdmissionAttemptArtifact>,
    obligation: OracleControlObligationV1,
    runner: ContentId<OracleControlRunnerArtifact>,
    qualification: ContentId<OracleMechanismQualificationReceiptArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleControlRunWire {
    schema_version: u16,
    attempt: ContentId<OracleAdmissionAttemptArtifact>,
    obligation: OracleControlObligationV1,
    runner: ContentId<OracleControlRunnerArtifact>,
    qualification: ContentId<OracleMechanismQualificationReceiptArtifact>,
}

impl OracleControlRunV1 {
    pub fn new(
        attempt: &OracleAdmissionAttemptV1,
        mechanisms: &OracleAdmissionMechanismCatalogV1,
        obligation: OracleControlObligationV1,
    ) -> Result<Self, OracleControlError> {
        if attempt.mechanisms() != mechanisms.identity()?
            || !attempt.required_controls().contains(&obligation)
        {
            return Err(OracleControlError::BindingMismatch);
        }
        let registration = mechanisms
            .registration(obligation.control())
            .ok_or(OracleControlError::BindingMismatch)?;
        if registration.mechanism() != obligation.mechanism() {
            return Err(OracleControlError::BindingMismatch);
        }
        let value = Self {
            schema_version: SCHEMA_V1,
            attempt: attempt.identity()?,
            obligation,
            runner: registration.runner(),
            qualification: registration.qualification(),
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn attempt(&self) -> ContentId<OracleAdmissionAttemptArtifact> {
        self.attempt
    }

    #[must_use]
    pub const fn obligation(&self) -> &OracleControlObligationV1 {
        &self.obligation
    }

    #[must_use]
    pub const fn runner(&self) -> ContentId<OracleControlRunnerArtifact> {
        self.runner
    }

    #[must_use]
    pub const fn qualification(&self) -> ContentId<OracleMechanismQualificationReceiptArtifact> {
        self.qualification
    }

    pub fn identity(&self) -> Result<ContentId<OracleControlRunArtifact>, OracleControlError> {
        self.validate()?;
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleControlError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleControlError::UnsupportedSchema);
        }
        Ok(())
    }
}

impl TryFrom<OracleControlRunWire> for OracleControlRunV1 {
    type Error = OracleControlError;

    fn try_from(wire: OracleControlRunWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            attempt: wire.attempt,
            obligation: wire.obligation,
            runner: wire.runner,
            qualification: wire.qualification,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleControlRunV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        OracleControlRunWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Worker execution identity prepared without starting the qualified mechanism.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleControlWorkerBindingV1 {
    job_id: JobId,
    attempt_id: AttemptId,
    contract: ContentId<JobContractArtifact>,
}

impl OracleControlWorkerBindingV1 {
    #[must_use]
    pub const fn new(
        job_id: JobId,
        attempt_id: AttemptId,
        contract: ContentId<JobContractArtifact>,
    ) -> Self {
        Self {
            job_id,
            attempt_id,
            contract,
        }
    }

    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<JobContractArtifact> {
        self.contract
    }
}

/// Exact dispatch committed before the qualified Worker mechanism may execute.
///
/// Runner and dispatch identities are intentionally distinct authority types.
///
/// ```compile_fail
/// use cairn_migration::{OracleControlDispatchArtifact, OracleControlRunnerArtifact};
/// use cairn_protocol::ContentId;
/// fn require_dispatch(_: ContentId<OracleControlDispatchArtifact>) {}
/// fn invalid(runner: ContentId<OracleControlRunnerArtifact>) {
///     require_dispatch(runner);
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleControlDispatchV1 {
    schema_version: u16,
    run: ContentId<OracleControlRunArtifact>,
    runner: ContentId<OracleControlRunnerArtifact>,
    worker: OracleControlWorkerBindingV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleControlDispatchWire {
    schema_version: u16,
    run: ContentId<OracleControlRunArtifact>,
    runner: ContentId<OracleControlRunnerArtifact>,
    worker: OracleControlWorkerBindingV1,
}

impl OracleControlDispatchV1 {
    pub fn new(
        run: &OracleControlRunV1,
        worker: OracleControlWorkerBindingV1,
    ) -> Result<Self, OracleControlError> {
        Ok(Self {
            schema_version: SCHEMA_V1,
            run: run.identity()?,
            runner: run.runner(),
            worker,
        })
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleControlRunArtifact> {
        self.run
    }

    #[must_use]
    pub const fn runner(&self) -> ContentId<OracleControlRunnerArtifact> {
        self.runner
    }

    #[must_use]
    pub const fn worker(&self) -> &OracleControlWorkerBindingV1 {
        &self.worker
    }

    pub fn identity(&self) -> Result<ContentId<OracleControlDispatchArtifact>, OracleControlError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleControlError::UnsupportedSchema);
        }
        derive_id(self)
    }

    pub fn validate_against(&self, run: &OracleControlRunV1) -> Result<(), OracleControlError> {
        if self.schema_version != SCHEMA_V1
            || self.run != run.identity()?
            || self.runner != run.runner()
        {
            return Err(OracleControlError::BindingMismatch);
        }
        Ok(())
    }
}

impl TryFrom<OracleControlDispatchWire> for OracleControlDispatchV1 {
    type Error = OracleControlError;

    fn try_from(wire: OracleControlDispatchWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(OracleControlError::UnsupportedSchema);
        }
        Ok(Self {
            schema_version: wire.schema_version,
            run: wire.run,
            runner: wire.runner,
            worker: wire.worker,
        })
    }
}

impl<'de> Deserialize<'de> for OracleControlDispatchV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        OracleControlDispatchWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical trusted observation produced by the qualified adapter for one exact dispatch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TrustedOracleControlObservationV1 {
    schema_version: u16,
    dispatch: ContentId<OracleControlDispatchArtifact>,
    run: ContentId<OracleControlRunArtifact>,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    result: OracleControlResultV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedOracleControlObservationWire {
    schema_version: u16,
    dispatch: ContentId<OracleControlDispatchArtifact>,
    run: ContentId<OracleControlRunArtifact>,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    result: OracleControlResultV1,
}

impl TrustedOracleControlObservationV1 {
    pub fn new(
        dispatch: &OracleControlDispatchV1,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        receipt: ExecutionReceipt,
        result: OracleControlResultV1,
    ) -> Result<Self, OracleControlError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            dispatch: dispatch.identity()?,
            run: dispatch.run,
            receipt_id,
            receipt,
            result,
        };
        value.validate_against(dispatch)?;
        Ok(value)
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleControlRunArtifact> {
        self.run
    }

    #[must_use]
    pub const fn receipt_id(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt_id
    }

    #[must_use]
    pub const fn receipt(&self) -> &ExecutionReceipt {
        &self.receipt
    }

    #[must_use]
    pub const fn result(&self) -> OracleControlResultV1 {
        self.result
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<TrustedOracleControlReceiptArtifact>, OracleControlError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleControlError::UnsupportedSchema);
        }
        derive_id(self)
    }

    pub fn validate_against(
        &self,
        dispatch: &OracleControlDispatchV1,
    ) -> Result<(), OracleControlError> {
        if self.schema_version != SCHEMA_V1
            || self.dispatch != dispatch.identity()?
            || self.run != dispatch.run
            || self.receipt.job_id() != dispatch.worker.job_id
            || self.receipt.attempt_id() != dispatch.worker.attempt_id
            || self.receipt.contract_id() != dispatch.worker.contract
        {
            return Err(OracleControlError::BindingMismatch);
        }
        let receipt_bytes = cairn_codec::to_vec(&self.receipt).map_err(codec)?;
        if ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes).map_err(codec)?
            != self.receipt_id
        {
            return Err(OracleControlError::BindingMismatch);
        }
        Ok(())
    }
}

impl TryFrom<TrustedOracleControlObservationWire> for TrustedOracleControlObservationV1 {
    type Error = OracleControlError;

    fn try_from(wire: TrustedOracleControlObservationWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            dispatch: wire.dispatch,
            run: wire.run,
            receipt_id: wire.receipt_id,
            receipt: wire.receipt,
            result: wire.result,
        };
        if value.schema_version != SCHEMA_V1 {
            return Err(OracleControlError::UnsupportedSchema);
        }
        let receipt_bytes = cairn_codec::to_vec(&value.receipt).map_err(codec)?;
        if ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes).map_err(codec)?
            != value.receipt_id
        {
            return Err(OracleControlError::BindingMismatch);
        }
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for TrustedOracleControlObservationV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        TrustedOracleControlObservationWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Controller-selected adapter for one qualified Oracle mechanism implementation.
pub trait OracleControlWorker {
    fn prepare(
        &mut self,
        run: &OracleControlRunV1,
    ) -> Result<OracleControlWorkerBindingV1, OracleControlWorkerError>;

    fn execute(
        &mut self,
        dispatch: &OracleControlDispatchV1,
    ) -> Result<TrustedOracleControlObservationV1, OracleControlWorkerError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum OracleControlWorkerError {
    #[error("Worker proved the Oracle control did not start: {0}")]
    NotStarted(String),
    #[error("Worker rejected the Oracle control: {0}")]
    Rejected(String),
    #[error("Worker Oracle control outcome is ambiguous: {0}")]
    Ambiguous(String),
}

#[derive(Debug, Error)]
pub enum OracleControlError {
    #[error("only Oracle control schema V1 is supported")]
    UnsupportedSchema,
    #[error("Oracle control authority binding changed")]
    BindingMismatch,
    #[error("Oracle framework rejected the control: {0}")]
    Framework(String),
    #[error("Oracle control codec failed: {0}")]
    Codec(String),
}

impl From<OracleFrameworkError> for OracleControlError {
    fn from(error: OracleFrameworkError) -> Self {
        Self::Framework(error.to_string())
    }
}

fn derive_id<T: Serialize, A: ContentType>(value: &T) -> Result<ContentId<A>, OracleControlError> {
    ContentId::derive(&cairn_codec::to_vec(value).map_err(codec)?).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> OracleControlError {
    OracleControlError::Codec(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OracleQualifiedMechanismRegistrationV1;

    fn id<A: ContentType>(label: &str) -> ContentId<A> {
        ContentId::derive(label.as_bytes()).expect("content identity")
    }

    #[test]
    fn qualification_receipt_is_canonical_and_registration_bound() {
        let receipt = OracleMechanismQualificationReceiptV1::new(
            OracleControlFamilyV1::Honest,
            id("mechanism"),
            id("runner"),
            id("execution evidence"),
        );
        let receipt_id = receipt.identity().expect("qualification identity");
        let registration = OracleQualifiedMechanismRegistrationV1::new(
            receipt.control(),
            receipt.mechanism(),
            receipt.runner(),
            receipt_id,
        );
        registration
            .validate_qualification(&receipt)
            .expect("exact qualification");
        let decoded: OracleMechanismQualificationReceiptV1 =
            cairn_codec::from_slice(&cairn_codec::to_vec(&receipt).expect("encode qualification"))
                .expect("decode qualification");
        assert_eq!(decoded, receipt);

        let changed = OracleMechanismQualificationReceiptV1::new(
            receipt.control(),
            receipt.mechanism(),
            id("another runner"),
            receipt.evidence(),
        );
        assert!(registration.validate_qualification(&changed).is_err());
        let mut non_v1 = serde_json::to_value(receipt).expect("qualification JSON");
        non_v1["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<OracleMechanismQualificationReceiptV1>(non_v1).is_err());
    }
}
