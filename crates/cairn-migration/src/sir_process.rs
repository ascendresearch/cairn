//! Typed one-shot boundary for an isolated SIR proposal process.

use std::collections::BTreeMap;

use cairn_protocol::{ContentId, ContentType, OperationId, SchemaVersion, SirRunId};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1, SirError,
    SirIntentHypothesisSetProposalArtifact, SirTaskBundleV1,
};

const RECORDED_INGRESS_PROTOCOL_SOURCE: &[u8] = include_bytes!("sir_process.rs");
const RECORDED_INGRESS_PROCESS_SOURCE: &[u8] = include_bytes!("../../cairn-sir/src/main.rs");

/// Exact implementation admitted at the recorded SIR process ingress.
pub enum SirProcessImplementationArtifact {}

impl ContentType for SirProcessImplementationArtifact {
    const DOMAIN: &'static str = "migration.sir-process-implementation.v1";
}

/// Canonical request sent over stdin to one isolated SIR process.
pub enum SirProcessRequestArtifact {}

impl ContentType for SirProcessRequestArtifact {
    const DOMAIN: &'static str = "migration.sir-process-request.v1";
}

/// Canonical successful terminal outcome emitted by one isolated SIR process.
pub enum SirProcessTerminalArtifact {}

impl ContentType for SirProcessTerminalArtifact {
    const DOMAIN: &'static str = "migration.sir-process-terminal.v1";
}

/// Frozen, materialized input for replaying a previously completed proposal through the process
/// authority boundary. It grants no public or restricted store handle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirProcessRequestV1 {
    schema_version: SchemaVersion,
    run_id: SirRunId,
    operation_id: OperationId,
    implementation: ContentId<SirProcessImplementationArtifact>,
    task_bundle: SirTaskBundleV1,
    recovery_input: IntentRecoveryInputV1,
    proposal: IntentHypothesisSetProposalV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirProcessRequestWire {
    schema_version: SchemaVersion,
    run_id: SirRunId,
    operation_id: OperationId,
    implementation: ContentId<SirProcessImplementationArtifact>,
    task_bundle: SirTaskBundleV1,
    recovery_input: IntentRecoveryInputV1,
    proposal: IntentHypothesisSetProposalV1,
}

impl SirProcessRequestV1 {
    /// Creates an exact materialized recorded-ingress request.
    ///
    /// # Errors
    ///
    /// Rejects task/input/proposal/citation bindings that do not close mechanically.
    pub fn new(
        run_id: SirRunId,
        operation_id: OperationId,
        task_bundle: SirTaskBundleV1,
        recovery_input: IntentRecoveryInputV1,
        proposal: IntentHypothesisSetProposalV1,
    ) -> Result<Self, SirProcessError> {
        let value = Self {
            schema_version: schema_v1(),
            run_id,
            operation_id,
            implementation: sir_recorded_ingress_implementation_id()?,
            task_bundle,
            recovery_input,
            proposal,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), SirProcessError> {
        if self.schema_version != schema_v1()
            || self.implementation != sir_recorded_ingress_implementation_id()?
        {
            return Err(SirProcessError::InvalidEnvelope);
        }
        let bundle_id = self.task_bundle.identity()?;
        if self.recovery_input.task_bundle() != bundle_id {
            return Err(SirProcessError::Binding("task bundle"));
        }
        let recovery_input_id = self.recovery_input.identity()?;
        if self.proposal.recovery_input() != recovery_input_id {
            return Err(SirProcessError::Binding("recovery input"));
        }
        self.proposal
            .submission()
            .validate_against_recovery_input(&self.recovery_input)?;
        validate_citations(&self.task_bundle, &self.proposal)
    }

    /// Derives the exact request identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(&self) -> Result<ContentId<SirProcessRequestArtifact>, SirProcessError> {
        let bytes = cairn_codec::to_vec(self)?;
        ContentId::derive(&bytes).map_err(|error| SirProcessError::Codec(error.to_string()))
    }
}

impl TryFrom<SirProcessRequestWire> for SirProcessRequestV1 {
    type Error = SirProcessError;

    fn try_from(wire: SirProcessRequestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            run_id: wire.run_id,
            operation_id: wire.operation_id,
            implementation: wire.implementation,
            task_bundle: wire.task_bundle,
            recovery_input: wire.recovery_input,
            proposal: wire.proposal,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for SirProcessRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirProcessRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Successful terminal outcome. Proposal bytes remain proposal-only after crossing this boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirProcessTerminalV1 {
    schema_version: SchemaVersion,
    run_id: SirRunId,
    operation_id: OperationId,
    implementation: ContentId<SirProcessImplementationArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    proposal: IntentHypothesisSetProposalV1,
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirProcessTerminalWire {
    schema_version: SchemaVersion,
    run_id: SirRunId,
    operation_id: OperationId,
    implementation: ContentId<SirProcessImplementationArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    proposal: IntentHypothesisSetProposalV1,
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
}

impl SirProcessTerminalV1 {
    fn from_request(request: &SirProcessRequestV1) -> Result<Self, SirProcessError> {
        let value = Self {
            schema_version: schema_v1(),
            run_id: request.run_id,
            operation_id: request.operation_id,
            implementation: request.implementation,
            recovery_input: request.recovery_input.identity()?,
            proposal: request.proposal.clone(),
            proposal_id: request.proposal.identity()?,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), SirProcessError> {
        if self.schema_version != schema_v1()
            || self.implementation != sir_recorded_ingress_implementation_id()?
            || self.proposal.recovery_input() != self.recovery_input
            || self.proposal.identity()? != self.proposal_id
        {
            return Err(SirProcessError::InvalidTerminal);
        }
        Ok(())
    }

    #[must_use]
    pub const fn proposal(&self) -> &IntentHypothesisSetProposalV1 {
        &self.proposal
    }

    #[must_use]
    pub const fn proposal_id(&self) -> ContentId<SirIntentHypothesisSetProposalArtifact> {
        self.proposal_id
    }

    /// Derives the exact terminal identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(&self) -> Result<ContentId<SirProcessTerminalArtifact>, SirProcessError> {
        let bytes = cairn_codec::to_vec(self)?;
        ContentId::derive(&bytes).map_err(|error| SirProcessError::Codec(error.to_string()))
    }
}

impl TryFrom<SirProcessTerminalWire> for SirProcessTerminalV1 {
    type Error = SirProcessError;

    fn try_from(wire: SirProcessTerminalWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            run_id: wire.run_id,
            operation_id: wire.operation_id,
            implementation: wire.implementation,
            recovery_input: wire.recovery_input,
            proposal: wire.proposal,
            proposal_id: wire.proposal_id,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for SirProcessTerminalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirProcessTerminalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Validates a canonical materialized SIR request and produces one terminal proposal outcome.
///
/// # Errors
///
/// Rejects any request whose exact V1 bindings, implementation, or citations are invalid.
pub fn process_recorded_sir_request(
    request: &SirProcessRequestV1,
) -> Result<SirProcessTerminalV1, SirProcessError> {
    request.validate()?;
    SirProcessTerminalV1::from_request(request)
}

/// Exact identity of the current recorded proposal-ingress implementation.
///
/// # Errors
///
/// Returns an error only if typed identity derivation fails.
pub fn sir_recorded_ingress_implementation_id()
-> Result<ContentId<SirProcessImplementationArtifact>, SirProcessError> {
    let material = cairn_codec::to_vec(&(
        RECORDED_INGRESS_PROTOCOL_SOURCE,
        RECORDED_INGRESS_PROCESS_SOURCE,
    ))?;
    ContentId::derive(&material).map_err(|error| SirProcessError::Codec(error.to_string()))
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("current V1 is a valid non-zero schema version")
}

fn validate_citations(
    task_bundle: &SirTaskBundleV1,
    proposal: &IntentHypothesisSetProposalV1,
) -> Result<(), SirProcessError> {
    let artifacts = task_bundle
        .artifacts()
        .iter()
        .map(|artifact| (artifact.path(), artifact.line_count()))
        .collect::<BTreeMap<_, _>>();
    for fact in proposal.submission().observed_facts() {
        for citation in fact.citations() {
            let Some(line_count) = artifacts.get(citation.path()) else {
                return Err(SirProcessError::Citation);
            };
            if citation.end_line().get() > line_count.get() {
                return Err(SirProcessError::Citation);
            }
        }
    }
    Ok(())
}

/// Fail-closed recorded SIR process boundary errors.
#[derive(Debug, Error)]
pub enum SirProcessError {
    #[error("SIR contract rejected: {0}")]
    Sir(#[from] SirError),
    #[error("canonical codec rejected the SIR process material: {0}")]
    Codec(String),
    #[error("invalid current-V1 SIR process request envelope")]
    InvalidEnvelope,
    #[error("SIR process request has an invalid {0} binding")]
    Binding(&'static str),
    #[error("SIR process proposal citation is outside the frozen task bundle")]
    Citation,
    #[error("invalid current-V1 SIR terminal outcome")]
    InvalidTerminal,
}

impl From<cairn_codec::CodecError> for SirProcessError {
    fn from(error: cairn_codec::CodecError) -> Self {
        Self::Codec(error.to_string())
    }
}
