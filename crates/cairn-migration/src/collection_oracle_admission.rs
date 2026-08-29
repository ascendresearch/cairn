//! Deterministic admission of the first claim-scoped collection Oracle capability.

use cairn_execution::ExecutionReceiptArtifact;
use cairn_protocol::{ContentId, ContentType, SchemaVersion};
use cairn_record::ContentStore;
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AssembledCollectionF32OracleCaseInput, CallAdapterExecutableArtifact,
    CollectionF32InvocationArtifact, CollectionOracleMechanismArtifact,
    CollectionOutputComparisonEvidenceArtifact, CollectionOutputComparisonV1,
    CollectionOutputOracleDecisionArtifact, CollectionOutputOracleDecisionV1,
    CollectionOutputOraclePolicyV1, MigrationIntentContractArtifact, PreparedCallAdapterInput,
    PreparedCollectionOutputComparisonEvidence, SirCallerClaimId, ValidatedCallAdapterExecution,
    collection_oracle_mechanism_id, materialize_collection_output_comparison,
};

const COLLECTION_ORACLE_ADMISSION_GATE_V1: &[u8] = include_bytes!("collection_oracle_admission.rs");

/// Deterministic proposal for the first local collection-output Oracle claim.
pub enum CollectionOracleClaimProposalArtifact {}

impl ContentType for CollectionOracleClaimProposalArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-claim-proposal.v1";
}

/// Exact deterministic gate that qualifies and freezes the local claim.
pub enum CollectionOracleAdmissionGateArtifact {}

impl ContentType for CollectionOracleAdmissionGateArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-admission-gate.v1";
}

/// Restricted qualification receipt for the first local claim.
pub enum CollectionOracleQualificationReceiptArtifact {}

impl ContentType for CollectionOracleQualificationReceiptArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-qualification-receipt.v1";
}

/// Public identity of a claim-scoped admitted Oracle capability.
pub enum AdmittedCollectionOracleClaimArtifact {}

impl ContentType for AdmittedCollectionOracleClaimArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-claim-admitted.v1";
}

/// Narrow semantic domain qualified by this first gate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOracleClaimDomainV1 {
    FiniteNormalF32StrictlyAboveThreshold,
}

/// Strength of the one admitted local relation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOracleClaimStrengthV1 {
    ExactOccurrenceMultisetAndReportedCount,
}

/// Closure boundary that prevents one local claim from masquerading as a full portfolio.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOracleClosureV1 {
    LocalClaimOnly,
}

/// Explicit limitations retained by the qualification receipt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOracleQualificationLimitV1 {
    FiniteNormalF32Only,
    HostAdapterOnly,
    NoCudaDeviceEvidence,
    NoPortfolioClosure,
    NoSafetyEvidence,
    NoTargetBuildOrDeviceEvidence,
}

/// Changes that invalidate this exact qualification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOracleRequalificationTriggerV1 {
    ControlExecutableChanged,
    DomainChanged,
    GateChanged,
    IntentDecisionChanged,
    MechanismChanged,
}

const QUALIFICATION_LIMITS: [CollectionOracleQualificationLimitV1; 6] = [
    CollectionOracleQualificationLimitV1::FiniteNormalF32Only,
    CollectionOracleQualificationLimitV1::HostAdapterOnly,
    CollectionOracleQualificationLimitV1::NoCudaDeviceEvidence,
    CollectionOracleQualificationLimitV1::NoPortfolioClosure,
    CollectionOracleQualificationLimitV1::NoSafetyEvidence,
    CollectionOracleQualificationLimitV1::NoTargetBuildOrDeviceEvidence,
];

const REQUALIFICATION_TRIGGERS: [CollectionOracleRequalificationTriggerV1; 5] = [
    CollectionOracleRequalificationTriggerV1::ControlExecutableChanged,
    CollectionOracleRequalificationTriggerV1::DomainChanged,
    CollectionOracleRequalificationTriggerV1::GateChanged,
    CollectionOracleRequalificationTriggerV1::IntentDecisionChanged,
    CollectionOracleRequalificationTriggerV1::MechanismChanged,
];

/// Proposed local claim. It has no admission outcome or candidate-verdict authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionOracleClaimProposalV1 {
    schema_version: SchemaVersion,
    decision: ContentId<CollectionOutputOracleDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    policy: CollectionOutputOraclePolicyV1,
    domain: CollectionOracleClaimDomainV1,
    strength: CollectionOracleClaimStrengthV1,
    mechanism: ContentId<CollectionOracleMechanismArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionOracleClaimProposalWire {
    schema_version: SchemaVersion,
    decision: ContentId<CollectionOutputOracleDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    policy: CollectionOutputOraclePolicyV1,
    domain: CollectionOracleClaimDomainV1,
    strength: CollectionOracleClaimStrengthV1,
    mechanism: ContentId<CollectionOracleMechanismArtifact>,
}

impl CollectionOracleClaimProposalV1 {
    #[must_use]
    pub const fn decision(&self) -> ContentId<CollectionOutputOracleDecisionArtifact> {
        self.decision
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.contract
    }

    #[must_use]
    pub const fn selection_claim(&self) -> &SirCallerClaimId {
        &self.selection_claim
    }

    #[must_use]
    pub const fn mechanism(&self) -> ContentId<CollectionOracleMechanismArtifact> {
        self.mechanism
    }

    /// Derives the exact proposal identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CollectionOracleClaimProposalArtifact>, CollectionOracleAdmissionError>
    {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), CollectionOracleAdmissionError> {
        if self.schema_version != schema_v1()
            || self.policy != CollectionOutputOraclePolicyV1::ExactMultisetAndCount
            || self.domain != CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold
            || self.strength
                != CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount
        {
            return Err(CollectionOracleAdmissionError::InvalidProposal);
        }
        Ok(())
    }
}

impl TryFrom<CollectionOracleClaimProposalWire> for CollectionOracleClaimProposalV1 {
    type Error = CollectionOracleAdmissionError;

    fn try_from(wire: CollectionOracleClaimProposalWire) -> Result<Self, Self::Error> {
        let proposal = Self {
            schema_version: wire.schema_version,
            decision: wire.decision,
            contract: wire.contract,
            selection_claim: wire.selection_claim,
            policy: wire.policy,
            domain: wire.domain,
            strength: wire.strength,
            mechanism: wire.mechanism,
        };
        proposal.validate()?;
        Ok(proposal)
    }
}

impl<'de> Deserialize<'de> for CollectionOracleClaimProposalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionOracleClaimProposalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact execution and comparison identities retained for one qualification control.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionOracleQualificationTrialV1 {
    invocation: ContentId<CollectionF32InvocationArtifact>,
    executable: ContentId<CallAdapterExecutableArtifact>,
    execution_receipt: ContentId<ExecutionReceiptArtifact>,
    comparison_evidence: ContentId<CollectionOutputComparisonEvidenceArtifact>,
    comparison: CollectionOutputComparisonV1,
}

/// Restricted receipt proving the exact gate observed one honest accept and one actual fault
/// rejection through the same invocation semantics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionOracleQualificationReceiptV1 {
    schema_version: SchemaVersion,
    proposal: ContentId<CollectionOracleClaimProposalArtifact>,
    mechanism: ContentId<CollectionOracleMechanismArtifact>,
    gate: ContentId<CollectionOracleAdmissionGateArtifact>,
    honest_reordered: CollectionOracleQualificationTrialV1,
    missing_occurrence: CollectionOracleQualificationTrialV1,
    limitations: Vec<CollectionOracleQualificationLimitV1>,
    requalification_triggers: Vec<CollectionOracleRequalificationTriggerV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionOracleQualificationReceiptWire {
    schema_version: SchemaVersion,
    proposal: ContentId<CollectionOracleClaimProposalArtifact>,
    mechanism: ContentId<CollectionOracleMechanismArtifact>,
    gate: ContentId<CollectionOracleAdmissionGateArtifact>,
    honest_reordered: CollectionOracleQualificationTrialV1,
    missing_occurrence: CollectionOracleQualificationTrialV1,
    limitations: Vec<CollectionOracleQualificationLimitV1>,
    requalification_triggers: Vec<CollectionOracleRequalificationTriggerV1>,
}

impl CollectionOracleQualificationReceiptV1 {
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CollectionOracleClaimProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn mechanism(&self) -> ContentId<CollectionOracleMechanismArtifact> {
        self.mechanism
    }

    #[must_use]
    pub const fn gate(&self) -> ContentId<CollectionOracleAdmissionGateArtifact> {
        self.gate
    }

    #[must_use]
    pub const fn honest_reordered(&self) -> &CollectionOracleQualificationTrialV1 {
        &self.honest_reordered
    }

    #[must_use]
    pub const fn missing_occurrence(&self) -> &CollectionOracleQualificationTrialV1 {
        &self.missing_occurrence
    }

    #[must_use]
    pub fn limitations(&self) -> &[CollectionOracleQualificationLimitV1] {
        &self.limitations
    }

    fn validate_structure(&self) -> Result<(), CollectionOracleAdmissionError> {
        if self.schema_version != schema_v1()
            || self.honest_reordered.comparison != CollectionOutputComparisonV1::Equivalent
            || self.missing_occurrence.comparison
                != CollectionOutputComparisonV1::ReportedCountMismatch
            || self.honest_reordered.invocation != self.missing_occurrence.invocation
            || self.honest_reordered.executable == self.missing_occurrence.executable
            || self.honest_reordered.execution_receipt == self.missing_occurrence.execution_receipt
            || self.honest_reordered.comparison_evidence
                == self.missing_occurrence.comparison_evidence
            || self.limitations != QUALIFICATION_LIMITS
            || self.requalification_triggers != REQUALIFICATION_TRIGGERS
        {
            return Err(CollectionOracleAdmissionError::InvalidQualificationReceipt);
        }
        Ok(())
    }

    /// Revalidates every proposal and current gate/mechanism edge.
    ///
    /// # Errors
    ///
    /// Rejects any mismatched or stale identity.
    pub fn validate_proposal(
        &self,
        proposal: &CollectionOracleClaimProposalV1,
    ) -> Result<(), CollectionOracleAdmissionError> {
        self.validate_structure()?;
        if self.proposal != proposal.identity()?
            || self.mechanism != proposal.mechanism
            || self.mechanism != collection_oracle_mechanism_id().map_err(admission)?
            || self.gate != collection_oracle_admission_gate_id()?
        {
            return Err(CollectionOracleAdmissionError::BindingMismatch);
        }
        Ok(())
    }
}

impl TryFrom<CollectionOracleQualificationReceiptWire> for CollectionOracleQualificationReceiptV1 {
    type Error = CollectionOracleAdmissionError;

    fn try_from(wire: CollectionOracleQualificationReceiptWire) -> Result<Self, Self::Error> {
        let receipt = Self {
            schema_version: wire.schema_version,
            proposal: wire.proposal,
            mechanism: wire.mechanism,
            gate: wire.gate,
            honest_reordered: wire.honest_reordered,
            missing_occurrence: wire.missing_occurrence,
            limitations: wire.limitations,
            requalification_triggers: wire.requalification_triggers,
        };
        receipt.validate_structure()?;
        Ok(receipt)
    }
}

impl<'de> Deserialize<'de> for CollectionOracleQualificationReceiptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionOracleQualificationReceiptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// First admitted Oracle capability. This is deliberately a local claim, not portfolio closure.
///
/// A local claim cannot substitute for the full admitted portfolio required by a release path.
///
/// ```compile_fail
/// use cairn_migration::AdmittedCollectionOracleClaimV1;
/// use cairn_verification::AdmittedOracleV1;
/// fn require_full(_: AdmittedOracleV1) {}
/// fn invalid(local: AdmittedCollectionOracleClaimV1) { require_full(local); }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdmittedCollectionOracleClaimV1 {
    schema_version: SchemaVersion,
    proposal: ContentId<CollectionOracleClaimProposalArtifact>,
    qualification_receipt: ContentId<CollectionOracleQualificationReceiptArtifact>,
    decision: ContentId<CollectionOutputOracleDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    mechanism: ContentId<CollectionOracleMechanismArtifact>,
    domain: CollectionOracleClaimDomainV1,
    strength: CollectionOracleClaimStrengthV1,
    closure: CollectionOracleClosureV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmittedCollectionOracleClaimWire {
    schema_version: SchemaVersion,
    proposal: ContentId<CollectionOracleClaimProposalArtifact>,
    qualification_receipt: ContentId<CollectionOracleQualificationReceiptArtifact>,
    decision: ContentId<CollectionOutputOracleDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    mechanism: ContentId<CollectionOracleMechanismArtifact>,
    domain: CollectionOracleClaimDomainV1,
    strength: CollectionOracleClaimStrengthV1,
    closure: CollectionOracleClosureV1,
}

impl AdmittedCollectionOracleClaimV1 {
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CollectionOracleClaimProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn decision(&self) -> ContentId<CollectionOutputOracleDecisionArtifact> {
        self.decision
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.contract
    }

    #[must_use]
    pub const fn selection_claim(&self) -> &SirCallerClaimId {
        &self.selection_claim
    }

    #[must_use]
    pub const fn qualification_receipt(
        &self,
    ) -> ContentId<CollectionOracleQualificationReceiptArtifact> {
        self.qualification_receipt
    }

    #[must_use]
    pub const fn mechanism(&self) -> ContentId<CollectionOracleMechanismArtifact> {
        self.mechanism
    }

    #[must_use]
    pub const fn domain(&self) -> CollectionOracleClaimDomainV1 {
        self.domain
    }

    #[must_use]
    pub const fn strength(&self) -> CollectionOracleClaimStrengthV1 {
        self.strength
    }

    #[must_use]
    pub const fn closure(&self) -> CollectionOracleClosureV1 {
        self.closure
    }

    /// Derives the exact public claim identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<AdmittedCollectionOracleClaimArtifact>, CollectionOracleAdmissionError>
    {
        derive_id(self)
    }

    /// Revalidates exact proposal and qualification receipt bindings.
    ///
    /// # Errors
    ///
    /// Rejects any changed authority edge.
    pub fn validate_inputs(
        &self,
        proposal: &CollectionOracleClaimProposalV1,
        receipt: &CollectionOracleQualificationReceiptV1,
    ) -> Result<(), CollectionOracleAdmissionError> {
        receipt.validate_proposal(proposal)?;
        let receipt_id = derive_id(receipt)?;
        if self.proposal != proposal.identity()?
            || self.qualification_receipt != receipt_id
            || self.decision != proposal.decision
            || self.contract != proposal.contract
            || self.selection_claim != proposal.selection_claim
            || self.mechanism != proposal.mechanism
            || self.domain != proposal.domain
            || self.strength != proposal.strength
            || self.closure != CollectionOracleClosureV1::LocalClaimOnly
        {
            return Err(CollectionOracleAdmissionError::BindingMismatch);
        }
        Ok(())
    }
}

impl TryFrom<AdmittedCollectionOracleClaimWire> for AdmittedCollectionOracleClaimV1 {
    type Error = CollectionOracleAdmissionError;

    fn try_from(wire: AdmittedCollectionOracleClaimWire) -> Result<Self, Self::Error> {
        if wire.schema_version != schema_v1()
            || wire.domain != CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold
            || wire.strength
                != CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount
            || wire.closure != CollectionOracleClosureV1::LocalClaimOnly
        {
            return Err(CollectionOracleAdmissionError::InvalidAdmittedClaim);
        }
        Ok(Self {
            schema_version: schema_v1(),
            proposal: wire.proposal,
            qualification_receipt: wire.qualification_receipt,
            decision: wire.decision,
            contract: wire.contract,
            selection_claim: wire.selection_claim,
            mechanism: wire.mechanism,
            domain: wire.domain,
            strength: wire.strength,
            closure: wire.closure,
        })
    }
}

impl<'de> Deserialize<'de> for AdmittedCollectionOracleClaimV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        AdmittedCollectionOracleClaimWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One already validated implementation execution supplied to the deterministic gate.
pub struct CollectionOracleQualificationExecution<'a, C: ContentStore> {
    pub adapter_input: &'a PreparedCallAdapterInput,
    pub execution: &'a ValidatedCallAdapterExecution,
    pub content: &'a C,
}

/// Canonical proposal, control comparisons, restricted receipt, and admitted local claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedAdmittedCollectionOracleClaim {
    proposal: CollectionOracleClaimProposalV1,
    proposal_bytes: Vec<u8>,
    proposal_id: ContentId<CollectionOracleClaimProposalArtifact>,
    honest_comparison: PreparedCollectionOutputComparisonEvidence,
    fault_comparison: PreparedCollectionOutputComparisonEvidence,
    receipt: CollectionOracleQualificationReceiptV1,
    receipt_bytes: Vec<u8>,
    receipt_id: ContentId<CollectionOracleQualificationReceiptArtifact>,
    claim: AdmittedCollectionOracleClaimV1,
    claim_bytes: Vec<u8>,
    claim_id: ContentId<AdmittedCollectionOracleClaimArtifact>,
}

impl PreparedAdmittedCollectionOracleClaim {
    #[must_use]
    pub const fn proposal(&self) -> &CollectionOracleClaimProposalV1 {
        &self.proposal
    }

    #[must_use]
    pub fn proposal_bytes(&self) -> &[u8] {
        &self.proposal_bytes
    }

    #[must_use]
    pub const fn proposal_id(&self) -> ContentId<CollectionOracleClaimProposalArtifact> {
        self.proposal_id
    }

    #[must_use]
    pub const fn honest_comparison(&self) -> &PreparedCollectionOutputComparisonEvidence {
        &self.honest_comparison
    }

    #[must_use]
    pub const fn fault_comparison(&self) -> &PreparedCollectionOutputComparisonEvidence {
        &self.fault_comparison
    }

    #[must_use]
    pub const fn receipt(&self) -> &CollectionOracleQualificationReceiptV1 {
        &self.receipt
    }

    #[must_use]
    pub fn receipt_bytes(&self) -> &[u8] {
        &self.receipt_bytes
    }

    #[must_use]
    pub const fn receipt_id(&self) -> ContentId<CollectionOracleQualificationReceiptArtifact> {
        self.receipt_id
    }

    #[must_use]
    pub const fn claim(&self) -> &AdmittedCollectionOracleClaimV1 {
        &self.claim
    }

    #[must_use]
    pub fn claim_bytes(&self) -> &[u8] {
        &self.claim_bytes
    }

    #[must_use]
    pub const fn claim_id(&self) -> ContentId<AdmittedCollectionOracleClaimArtifact> {
        self.claim_id
    }
}

/// Mechanically derives the local claim proposal from the admitted intent decision.
///
/// A proposal cannot recursively grant itself intent authority.
///
/// ```compile_fail
/// use cairn_migration::{
///     CollectionOracleClaimProposalV1, prepare_collection_oracle_claim_proposal,
/// };
/// fn invalid(proposal: &CollectionOracleClaimProposalV1) {
///     let _ = prepare_collection_oracle_claim_proposal(proposal);
/// }
/// ```
///
/// # Errors
///
/// Rejects any policy other than the currently qualified unordered occurrence contract.
pub fn prepare_collection_oracle_claim_proposal(
    decision: &CollectionOutputOracleDecisionV1,
) -> Result<CollectionOracleClaimProposalV1, CollectionOracleAdmissionError> {
    let proposal = CollectionOracleClaimProposalV1 {
        schema_version: schema_v1(),
        decision: decision.identity().map_err(admission)?,
        contract: decision.contract(),
        selection_claim: decision.selection_claim().clone(),
        policy: decision.policy(),
        domain: CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
        strength: CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
        mechanism: collection_oracle_mechanism_id().map_err(admission)?,
    };
    proposal.validate()?;
    Ok(proposal)
}

/// Returns the exact source identity of the deterministic local admission gate.
///
/// # Errors
///
/// Returns an error only if typed identity derivation fails.
pub fn collection_oracle_admission_gate_id()
-> Result<ContentId<CollectionOracleAdmissionGateArtifact>, CollectionOracleAdmissionError> {
    ContentId::derive(COLLECTION_ORACLE_ADMISSION_GATE_V1).map_err(admission)
}

/// Recomputes the honest and fault implementation observations and freezes one local admitted
/// Oracle claim.
///
/// # Errors
///
/// Rejects non-identical cases, shared executables, false rejection of the honest implementation,
/// failure to detect exactly one missing occurrence, or any identity/binding inconsistency.
pub fn prepare_admitted_collection_oracle_claim<C1: ContentStore, C2: ContentStore>(
    decision: &CollectionOutputOracleDecisionV1,
    case: &AssembledCollectionF32OracleCaseInput,
    honest: &CollectionOracleQualificationExecution<'_, C1>,
    fault: &CollectionOracleQualificationExecution<'_, C2>,
) -> Result<PreparedAdmittedCollectionOracleClaim, CollectionOracleAdmissionError> {
    let proposal = prepare_collection_oracle_claim_proposal(decision)?;
    if case.invocation().decision() != proposal.decision
        || case.invocation().contract() != proposal.contract
        || case.invocation().selection_claim() != &proposal.selection_claim
    {
        return Err(CollectionOracleAdmissionError::BindingMismatch);
    }
    validate_adapter_input(case, honest.adapter_input)?;
    validate_adapter_input(case, fault.adapter_input)?;
    let honest_executable = honest.adapter_input.request().executable();
    let fault_executable = fault.adapter_input.request().executable();
    if honest_executable == fault_executable
        || honest.execution.receipt_id() == fault.execution.receipt_id()
    {
        return Err(CollectionOracleAdmissionError::ControlIndependence);
    }

    let honest_comparison =
        materialize_collection_output_comparison(case, decision, honest.execution, honest.content)
            .map_err(admission)?;
    if honest_comparison.evidence().comparison() != CollectionOutputComparisonV1::Equivalent {
        return Err(CollectionOracleAdmissionError::HonestControlRejected);
    }
    let fault_comparison =
        materialize_collection_output_comparison(case, decision, fault.execution, fault.content)
            .map_err(admission)?;
    if fault_comparison.evidence().comparison()
        != CollectionOutputComparisonV1::ReportedCountMismatch
        || !is_exactly_one_missing_occurrence(case, fault_comparison.observed())
    {
        return Err(CollectionOracleAdmissionError::FaultControlAccepted);
    }

    let proposal_bytes = cairn_codec::to_vec(&proposal).map_err(codec)?;
    let proposal_id = proposal.identity()?;
    let receipt = CollectionOracleQualificationReceiptV1 {
        schema_version: schema_v1(),
        proposal: proposal_id,
        mechanism: proposal.mechanism,
        gate: collection_oracle_admission_gate_id()?,
        honest_reordered: qualification_trial(
            case,
            honest_executable,
            honest.execution,
            &honest_comparison,
        ),
        missing_occurrence: qualification_trial(
            case,
            fault_executable,
            fault.execution,
            &fault_comparison,
        ),
        limitations: QUALIFICATION_LIMITS.to_vec(),
        requalification_triggers: REQUALIFICATION_TRIGGERS.to_vec(),
    };
    receipt.validate_proposal(&proposal)?;
    let receipt_bytes = cairn_codec::to_vec(&receipt).map_err(codec)?;
    let receipt_id = derive_id(&receipt)?;
    let claim = AdmittedCollectionOracleClaimV1 {
        schema_version: schema_v1(),
        proposal: proposal_id,
        qualification_receipt: receipt_id,
        decision: proposal.decision,
        contract: proposal.contract,
        selection_claim: proposal.selection_claim.clone(),
        mechanism: proposal.mechanism,
        domain: proposal.domain,
        strength: proposal.strength,
        closure: CollectionOracleClosureV1::LocalClaimOnly,
    };
    claim.validate_inputs(&proposal, &receipt)?;
    let claim_bytes = cairn_codec::to_vec(&claim).map_err(codec)?;
    let claim_id = derive_id(&claim)?;
    Ok(PreparedAdmittedCollectionOracleClaim {
        proposal,
        proposal_bytes,
        proposal_id,
        honest_comparison,
        fault_comparison,
        receipt,
        receipt_bytes,
        receipt_id,
        claim,
        claim_bytes,
        claim_id,
    })
}

fn validate_adapter_input(
    case: &AssembledCollectionF32OracleCaseInput,
    input: &PreparedCallAdapterInput,
) -> Result<(), CollectionOracleAdmissionError> {
    if input.request().source_input_bundle() != case.input_bundle_id()
        || input.request().invocation()
            != (crate::CorpusInvocationIdentityV1::CollectionOutput {
                manifest: case.invocation_id(),
            })
    {
        return Err(CollectionOracleAdmissionError::BindingMismatch);
    }
    Ok(())
}

fn qualification_trial(
    case: &AssembledCollectionF32OracleCaseInput,
    executable: ContentId<CallAdapterExecutableArtifact>,
    execution: &ValidatedCallAdapterExecution,
    comparison: &PreparedCollectionOutputComparisonEvidence,
) -> CollectionOracleQualificationTrialV1 {
    CollectionOracleQualificationTrialV1 {
        invocation: case.invocation_id(),
        executable,
        execution_receipt: execution.receipt_id(),
        comparison_evidence: comparison.id(),
        comparison: comparison.evidence().comparison(),
    }
}

fn is_exactly_one_missing_occurrence(
    case: &AssembledCollectionF32OracleCaseInput,
    observed: &crate::ObservedCollectionOracleOutputV1,
) -> bool {
    if usize::try_from(observed.reported_count().get()).ok() != Some(observed.elements().len())
        || observed.elements().len().checked_add(1) != Some(case.expected().elements().len())
    {
        return false;
    }
    let mut remaining = case.expected().elements().to_vec();
    for element in observed.elements() {
        let Some(position) = remaining.iter().position(|candidate| candidate == element) else {
            return false;
        };
        remaining.remove(position);
    }
    remaining.len() == 1
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CollectionOracleAdmissionError {
    #[error("collection Oracle claim proposal is invalid for the current qualified scope")]
    InvalidProposal,
    #[error("collection Oracle qualification receipt is structurally invalid")]
    InvalidQualificationReceipt,
    #[error("admitted collection Oracle claim is structurally invalid")]
    InvalidAdmittedClaim,
    #[error("collection Oracle admission identity or authority binding mismatch")]
    BindingMismatch,
    #[error("honest and fault controls must use distinct executable and receipt identities")]
    ControlIndependence,
    #[error("the honest reordered implementation was rejected")]
    HonestControlRejected,
    #[error("the missing-occurrence implementation fault was not detected exactly")]
    FaultControlAccepted,
    #[error("collection Oracle admission dependency failed: {0}")]
    Dependency(String),
    #[error("collection Oracle admission codec failed: {0}")]
    Codec(String),
}

fn derive_id<T: ContentType>(
    value: &impl Serialize,
) -> Result<ContentId<T>, CollectionOracleAdmissionError> {
    let bytes = cairn_codec::to_vec(value).map_err(codec)?;
    ContentId::derive(&bytes).map_err(codec)
}

fn admission(error: impl std::fmt::Display) -> CollectionOracleAdmissionError {
    CollectionOracleAdmissionError::Dependency(error.to_string())
}

fn codec(error: impl std::fmt::Display) -> CollectionOracleAdmissionError {
    CollectionOracleAdmissionError::Codec(error.to_string())
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("current V1 is a valid non-zero schema version")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(policy: CollectionOutputOraclePolicyV1) -> CollectionOutputOracleDecisionV1 {
        CollectionOutputOracleDecisionV1::new(
            ContentId::<MigrationIntentContractArtifact>::derive(b"test intent contract")
                .expect("contract"),
            SirCallerClaimId::new("copies-strictly-above").expect("claim"),
            policy,
        )
    }

    #[test]
    fn proposal_is_strict_current_v1_and_only_covers_qualified_policy() {
        let proposal = prepare_collection_oracle_claim_proposal(&decision(
            CollectionOutputOraclePolicyV1::ExactMultisetAndCount,
        ))
        .expect("proposal");
        let bytes = cairn_codec::to_vec(&proposal).expect("proposal bytes");
        assert_eq!(
            cairn_codec::from_slice::<CollectionOracleClaimProposalV1>(&bytes)
                .expect("strict round trip"),
            proposal
        );
        let mut value = serde_json::to_value(&proposal).expect("proposal JSON");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<CollectionOracleClaimProposalV1>(value).is_err());
        let mut value = serde_json::to_value(&proposal).expect("proposal JSON");
        value["legacy_portfolio"] = serde_json::json!(true);
        assert!(serde_json::from_value::<CollectionOracleClaimProposalV1>(value).is_err());
        assert_eq!(
            prepare_collection_oracle_claim_proposal(&decision(
                CollectionOutputOraclePolicyV1::ExactSequenceAndCount,
            )),
            Err(CollectionOracleAdmissionError::InvalidProposal)
        );
    }
}
