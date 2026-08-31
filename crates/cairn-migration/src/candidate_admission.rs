//! Claim-scoped, model-free Candidate Admission over the exact admitted Oracle contract.
#![allow(clippy::missing_errors_doc)]

use std::collections::{HashMap, HashSet};

use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    CandidateOracleContractArtifact, CandidateOracleContractV1, CandidateProposalArtifact,
    CandidateProposalV1, OracleClaimArtifact, OraclePlaneV1, OracleWorkItemArtifact,
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
    CandidateControlImplementationArtifact,
    "migration.candidate-control-implementation.v1"
);
artifact!(
    CandidateQualifiedMechanismArtifact,
    "migration.candidate-qualified-mechanism.v1"
);
artifact!(
    CandidateMechanismCatalogArtifact,
    "migration.candidate-mechanism-catalog.v1"
);
artifact!(
    CandidateAdmissionAttemptArtifact,
    "migration.candidate-admission-attempt.v1"
);
artifact!(
    TrustedCandidateControlReceiptArtifact,
    "migration.trusted-candidate-control-receipt.v1"
);
artifact!(
    CandidateAdmissionEvidenceArtifact,
    "migration.candidate-admission-evidence.v1"
);
artifact!(
    CandidateAdmissionOutcomeArtifact,
    "migration.candidate-admission-outcome.v1"
);

/// Independent control families mechanically required by admitted Oracle work-item plane.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateControlFamilyV1 {
    SourceBuild,
    StaticAnalysis,
    ExecuteObservation,
    SemanticComparison,
    Safety,
    Performance,
}

/// Provenance class allowed to qualify a Candidate control implementation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateMechanismProvenanceV1 {
    Controller,
    Worker,
}

/// One product-qualified mechanism. Agent/model identity is deliberately absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateQualifiedMechanismV1 {
    schema_version: u16,
    family: CandidateControlFamilyV1,
    implementation: ContentId<CandidateControlImplementationArtifact>,
    provenance: CandidateMechanismProvenanceV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateQualifiedMechanismWire {
    schema_version: u16,
    family: CandidateControlFamilyV1,
    implementation: ContentId<CandidateControlImplementationArtifact>,
    provenance: CandidateMechanismProvenanceV1,
}

impl CandidateQualifiedMechanismV1 {
    #[must_use]
    pub fn new(
        family: CandidateControlFamilyV1,
        implementation: ContentId<CandidateControlImplementationArtifact>,
        provenance: CandidateMechanismProvenanceV1,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            family,
            implementation,
            provenance,
        }
    }
    #[must_use]
    pub const fn family(&self) -> CandidateControlFamilyV1 {
        self.family
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateQualifiedMechanismArtifact>, CandidateAdmissionError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateAdmissionError::UnsupportedSchema);
        }
        derive_id(self)
    }
}

impl TryFrom<CandidateQualifiedMechanismWire> for CandidateQualifiedMechanismV1 {
    type Error = CandidateAdmissionError;

    fn try_from(wire: CandidateQualifiedMechanismWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(CandidateAdmissionError::UnsupportedSchema);
        }
        Ok(Self {
            schema_version: wire.schema_version,
            family: wire.family,
            implementation: wire.implementation,
            provenance: wire.provenance,
        })
    }
}

impl<'de> Deserialize<'de> for CandidateQualifiedMechanismV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CandidateQualifiedMechanismWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Complete exact mechanism inventory for the current Candidate admission attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateMechanismCatalogV1 {
    schema_version: u16,
    mechanisms: Vec<CandidateQualifiedMechanismV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateMechanismCatalogWire {
    schema_version: u16,
    mechanisms: Vec<CandidateQualifiedMechanismV1>,
}

impl CandidateMechanismCatalogV1 {
    pub fn new(
        mut mechanisms: Vec<CandidateQualifiedMechanismV1>,
    ) -> Result<Self, CandidateAdmissionError> {
        mechanisms.sort_by_key(CandidateQualifiedMechanismV1::family);
        let value = Self {
            schema_version: SCHEMA_V1,
            mechanisms,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateMechanismCatalogArtifact>, CandidateAdmissionError> {
        self.validate()?;
        derive_id(self)
    }
    #[must_use]
    pub fn mechanisms(&self) -> &[CandidateQualifiedMechanismV1] {
        &self.mechanisms
    }
    fn mechanism(
        &self,
        family: CandidateControlFamilyV1,
    ) -> Option<ContentId<CandidateQualifiedMechanismArtifact>> {
        self.mechanisms
            .iter()
            .find(|value| value.family == family)
            .and_then(|value| value.identity().ok())
    }
    fn validate(&self) -> Result<(), CandidateAdmissionError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateAdmissionError::UnsupportedSchema);
        }
        if self.mechanisms.is_empty()
            || self
                .mechanisms
                .windows(2)
                .any(|pair| pair[0].family >= pair[1].family)
            || self
                .mechanisms
                .iter()
                .any(|value| value.identity().is_err())
        {
            return Err(CandidateAdmissionError::MechanismCatalogDrift);
        }
        Ok(())
    }
}

impl TryFrom<CandidateMechanismCatalogWire> for CandidateMechanismCatalogV1 {
    type Error = CandidateAdmissionError;
    fn try_from(wire: CandidateMechanismCatalogWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            mechanisms: wire.mechanisms,
        };
        value.validate()?;
        Ok(value)
    }
}
impl<'de> Deserialize<'de> for CandidateMechanismCatalogV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CandidateMechanismCatalogWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One exact item × control obligation; no Agent is asked to remember this matrix.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateControlObligationV1 {
    claim: ContentId<OracleClaimArtifact>,
    item: ContentId<OracleWorkItemArtifact>,
    plane: OraclePlaneV1,
    family: CandidateControlFamilyV1,
    mechanism: ContentId<CandidateQualifiedMechanismArtifact>,
}

impl CandidateControlObligationV1 {
    #[must_use]
    pub const fn claim(&self) -> ContentId<OracleClaimArtifact> {
        self.claim
    }
    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }
    #[must_use]
    pub const fn plane(&self) -> OraclePlaneV1 {
        self.plane
    }
    #[must_use]
    pub const fn family(&self) -> CandidateControlFamilyV1 {
        self.family
    }
    #[must_use]
    pub const fn mechanism(&self) -> ContentId<CandidateQualifiedMechanismArtifact> {
        self.mechanism
    }
}

/// Complete mechanically expanded attempt for one Candidate and admitted contract.
///
/// Its identity is distinct from the preceding build authority.
///
/// ```compile_fail
/// use cairn_migration::{CandidateAdmissionAttemptArtifact, CandidateBuildRequestArtifact};
/// use cairn_protocol::ContentId;
/// fn require_attempt(_: ContentId<CandidateAdmissionAttemptArtifact>) {}
/// fn invalid(build: ContentId<CandidateBuildRequestArtifact>) { require_attempt(build); }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateAdmissionAttemptV1 {
    schema_version: u16,
    contract: ContentId<CandidateOracleContractArtifact>,
    proposal: ContentId<CandidateProposalArtifact>,
    mechanisms: ContentId<CandidateMechanismCatalogArtifact>,
    obligations: Vec<CandidateControlObligationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateAdmissionAttemptWire {
    schema_version: u16,
    contract: ContentId<CandidateOracleContractArtifact>,
    proposal: ContentId<CandidateProposalArtifact>,
    mechanisms: ContentId<CandidateMechanismCatalogArtifact>,
    obligations: Vec<CandidateControlObligationV1>,
}

impl CandidateAdmissionAttemptV1 {
    pub fn new(
        contract: &CandidateOracleContractV1,
        proposal: &CandidateProposalV1,
        mechanisms: &CandidateMechanismCatalogV1,
    ) -> Result<Self, CandidateAdmissionError> {
        let contract_id = contract.identity().map_err(binding)?;
        if proposal.oracle_contract() != contract_id {
            return Err(CandidateAdmissionError::BindingMismatch);
        }
        let mut obligations = Vec::new();
        for claim in contract.admitted_claims() {
            for entry in claim.entries() {
                let item = entry.item().identity().map_err(binding)?;
                for family in required_controls(entry.item().plane()) {
                    obligations.push(CandidateControlObligationV1 {
                        claim: claim.claim(),
                        item,
                        plane: entry.item().plane(),
                        family,
                        mechanism: mechanisms
                            .mechanism(family)
                            .ok_or(CandidateAdmissionError::MissingMechanism(family))?,
                    });
                }
            }
        }
        obligations.sort_by_key(obligation_key);
        let value = Self {
            schema_version: SCHEMA_V1,
            contract: contract_id,
            proposal: proposal.identity().map_err(binding)?,
            mechanisms: mechanisms.identity()?,
            obligations,
        };
        value.validate_structure()?;
        Ok(value)
    }
    #[must_use]
    pub const fn contract(&self) -> ContentId<CandidateOracleContractArtifact> {
        self.contract
    }
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CandidateProposalArtifact> {
        self.proposal
    }
    #[must_use]
    pub const fn mechanisms(&self) -> ContentId<CandidateMechanismCatalogArtifact> {
        self.mechanisms
    }
    #[must_use]
    pub fn obligations(&self) -> &[CandidateControlObligationV1] {
        &self.obligations
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateAdmissionAttemptArtifact>, CandidateAdmissionError> {
        self.validate_structure()?;
        derive_id(self)
    }
    fn validate_against(
        &self,
        contract: &CandidateOracleContractV1,
        proposal: &CandidateProposalV1,
        mechanisms: &CandidateMechanismCatalogV1,
    ) -> Result<(), CandidateAdmissionError> {
        if self != &Self::new(contract, proposal, mechanisms)? {
            return Err(CandidateAdmissionError::BindingMismatch);
        }
        Ok(())
    }
    fn validate_structure(&self) -> Result<(), CandidateAdmissionError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateAdmissionError::UnsupportedSchema);
        }
        if self.obligations.is_empty()
            || self
                .obligations
                .windows(2)
                .any(|pair| obligation_key(&pair[0]) >= obligation_key(&pair[1]))
        {
            return Err(CandidateAdmissionError::AttemptDrift);
        }
        Ok(())
    }
}
impl TryFrom<CandidateAdmissionAttemptWire> for CandidateAdmissionAttemptV1 {
    type Error = CandidateAdmissionError;
    fn try_from(wire: CandidateAdmissionAttemptWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            contract: wire.contract,
            proposal: wire.proposal,
            mechanisms: wire.mechanisms,
            obligations: wire.obligations,
        };
        value.validate_structure()?;
        Ok(value)
    }
}
impl<'de> Deserialize<'de> for CandidateAdmissionAttemptV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CandidateAdmissionAttemptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateControlResultV1 {
    Passed,
    Failed,
    Unavailable,
}

/// Controller-validated observation from one qualified mechanism.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateControlReceiptV1 {
    item: ContentId<OracleWorkItemArtifact>,
    family: CandidateControlFamilyV1,
    mechanism: ContentId<CandidateQualifiedMechanismArtifact>,
    receipt: ContentId<TrustedCandidateControlReceiptArtifact>,
    result: CandidateControlResultV1,
}
impl CandidateControlReceiptV1 {
    #[must_use]
    pub const fn new(
        item: ContentId<OracleWorkItemArtifact>,
        family: CandidateControlFamilyV1,
        mechanism: ContentId<CandidateQualifiedMechanismArtifact>,
        receipt: ContentId<TrustedCandidateControlReceiptArtifact>,
        result: CandidateControlResultV1,
    ) -> Self {
        Self {
            item,
            family,
            mechanism,
            receipt,
            result,
        }
    }
    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }
    #[must_use]
    pub const fn family(&self) -> CandidateControlFamilyV1 {
        self.family
    }
    #[must_use]
    pub const fn receipt(&self) -> ContentId<TrustedCandidateControlReceiptArtifact> {
        self.receipt
    }
    #[must_use]
    pub const fn result(&self) -> CandidateControlResultV1 {
        self.result
    }
}

/// Exact trusted receipt set for one frozen attempt. Missing controls remain unresolved.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateAdmissionEvidenceV1 {
    schema_version: u16,
    attempt: ContentId<CandidateAdmissionAttemptArtifact>,
    receipts: Vec<CandidateControlReceiptV1>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateAdmissionEvidenceWire {
    schema_version: u16,
    attempt: ContentId<CandidateAdmissionAttemptArtifact>,
    receipts: Vec<CandidateControlReceiptV1>,
}
impl CandidateAdmissionEvidenceV1 {
    pub fn new(
        attempt: &CandidateAdmissionAttemptV1,
        mut receipts: Vec<CandidateControlReceiptV1>,
    ) -> Result<Self, CandidateAdmissionError> {
        receipts.sort_by_key(receipt_key);
        let value = Self {
            schema_version: SCHEMA_V1,
            attempt: attempt.identity()?,
            receipts,
        };
        value.validate_against(attempt)?;
        Ok(value)
    }
    #[must_use]
    pub fn receipts(&self) -> &[CandidateControlReceiptV1] {
        &self.receipts
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateAdmissionEvidenceArtifact>, CandidateAdmissionError> {
        self.validate_structure()?;
        derive_id(self)
    }
    fn validate_against(
        &self,
        attempt: &CandidateAdmissionAttemptV1,
    ) -> Result<(), CandidateAdmissionError> {
        self.validate_structure()?;
        if self.attempt != attempt.identity()?
            || self.receipts.iter().any(|receipt| {
                !attempt.obligations.iter().any(|obligation| {
                    obligation.item == receipt.item
                        && obligation.family == receipt.family
                        && obligation.mechanism == receipt.mechanism
                })
            })
        {
            return Err(CandidateAdmissionError::EvidenceDrift);
        }
        Ok(())
    }
    fn validate_structure(&self) -> Result<(), CandidateAdmissionError> {
        if self.schema_version != SCHEMA_V1
            || self
                .receipts
                .windows(2)
                .any(|pair| receipt_key(&pair[0]) >= receipt_key(&pair[1]))
            || self
                .receipts
                .iter()
                .map(|value| value.receipt)
                .collect::<HashSet<_>>()
                .len()
                != self.receipts.len()
        {
            return Err(CandidateAdmissionError::EvidenceDrift);
        }
        Ok(())
    }
}
impl TryFrom<CandidateAdmissionEvidenceWire> for CandidateAdmissionEvidenceV1 {
    type Error = CandidateAdmissionError;
    fn try_from(wire: CandidateAdmissionEvidenceWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            attempt: wire.attempt,
            receipts: wire.receipts,
        };
        value.validate_structure()?;
        Ok(value)
    }
}
impl<'de> Deserialize<'de> for CandidateAdmissionEvidenceV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CandidateAdmissionEvidenceWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateClaimStatusV1 {
    Admitted,
    Partial,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateClaimOutcomeV1 {
    claim: ContentId<OracleClaimArtifact>,
    status: CandidateClaimStatusV1,
    admitted_items: Vec<ContentId<OracleWorkItemArtifact>>,
    unresolved_items: Vec<ContentId<OracleWorkItemArtifact>>,
    rejected_items: Vec<ContentId<OracleWorkItemArtifact>>,
}
impl CandidateClaimOutcomeV1 {
    #[must_use]
    pub const fn claim(&self) -> ContentId<OracleClaimArtifact> {
        self.claim
    }
    #[must_use]
    pub const fn status(&self) -> CandidateClaimStatusV1 {
        self.status
    }
    #[must_use]
    pub fn admitted_items(&self) -> &[ContentId<OracleWorkItemArtifact>] {
        &self.admitted_items
    }
    #[must_use]
    pub fn unresolved_items(&self) -> &[ContentId<OracleWorkItemArtifact>] {
        &self.unresolved_items
    }
    #[must_use]
    pub fn rejected_items(&self) -> &[ContentId<OracleWorkItemArtifact>] {
        &self.rejected_items
    }
}

/// Model-free Candidate outcome recomputed from the complete obligation matrix and receipts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAdmissionOutcomeV1 {
    schema_version: u16,
    attempt: ContentId<CandidateAdmissionAttemptArtifact>,
    evidence: ContentId<CandidateAdmissionEvidenceArtifact>,
    proposal: ContentId<CandidateProposalArtifact>,
    claims: Vec<CandidateClaimOutcomeV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateAdmissionOutcomeWire {
    schema_version: u16,
    attempt: ContentId<CandidateAdmissionAttemptArtifact>,
    evidence: ContentId<CandidateAdmissionEvidenceArtifact>,
    proposal: ContentId<CandidateProposalArtifact>,
    claims: Vec<CandidateClaimOutcomeV1>,
}

impl CandidateAdmissionOutcomeV1 {
    #[must_use]
    pub const fn evidence(&self) -> ContentId<CandidateAdmissionEvidenceArtifact> {
        self.evidence
    }
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CandidateProposalArtifact> {
        self.proposal
    }
    #[must_use]
    pub fn claims(&self) -> &[CandidateClaimOutcomeV1] {
        &self.claims
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateAdmissionOutcomeArtifact>, CandidateAdmissionError> {
        self.validate()?;
        derive_id(self)
    }

    fn validate(&self) -> Result<(), CandidateAdmissionError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateAdmissionError::UnsupportedSchema);
        }
        if self.claims.is_empty()
            || self
                .claims
                .windows(2)
                .any(|pair| pair[0].claim.to_wire() >= pair[1].claim.to_wire())
        {
            return Err(CandidateAdmissionError::OutcomeDrift);
        }
        for claim in &self.claims {
            let canonical = |values: &[ContentId<OracleWorkItemArtifact>]| {
                !values
                    .windows(2)
                    .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
            };
            let all = claim
                .admitted_items
                .iter()
                .chain(&claim.unresolved_items)
                .chain(&claim.rejected_items)
                .copied()
                .collect::<HashSet<_>>();
            let count = claim.admitted_items.len()
                + claim.unresolved_items.len()
                + claim.rejected_items.len();
            let status_matches = match claim.status {
                CandidateClaimStatusV1::Admitted => {
                    !claim.admitted_items.is_empty()
                        && claim.unresolved_items.is_empty()
                        && claim.rejected_items.is_empty()
                }
                CandidateClaimStatusV1::Partial => {
                    !claim.unresolved_items.is_empty() && claim.rejected_items.is_empty()
                }
                CandidateClaimStatusV1::Rejected => !claim.rejected_items.is_empty(),
            };
            if !canonical(&claim.admitted_items)
                || !canonical(&claim.unresolved_items)
                || !canonical(&claim.rejected_items)
                || all.len() != count
                || !status_matches
            {
                return Err(CandidateAdmissionError::OutcomeDrift);
            }
        }
        Ok(())
    }
}

impl TryFrom<CandidateAdmissionOutcomeWire> for CandidateAdmissionOutcomeV1 {
    type Error = CandidateAdmissionError;
    fn try_from(wire: CandidateAdmissionOutcomeWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            attempt: wire.attempt,
            evidence: wire.evidence,
            proposal: wire.proposal,
            claims: wire.claims,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateAdmissionOutcomeV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CandidateAdmissionOutcomeWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Independently recomputes per-claim Candidate status. Any failed control rejects its item;
/// absent/unavailable controls remain explicit partial evidence.
pub fn recompute_candidate_admission(
    contract: &CandidateOracleContractV1,
    proposal: &CandidateProposalV1,
    mechanisms: &CandidateMechanismCatalogV1,
    attempt: &CandidateAdmissionAttemptV1,
    evidence: &CandidateAdmissionEvidenceV1,
) -> Result<CandidateAdmissionOutcomeV1, CandidateAdmissionError> {
    attempt.validate_against(contract, proposal, mechanisms)?;
    evidence.validate_against(attempt)?;
    let receipt_map = evidence
        .receipts
        .iter()
        .map(|receipt| ((receipt.item, receipt.family), receipt.result))
        .collect::<HashMap<_, _>>();
    let mut claims = Vec::new();
    for claim in contract.admitted_claims() {
        let mut admitted = Vec::new();
        let mut unresolved = Vec::new();
        let mut rejected = Vec::new();
        for entry in claim.entries() {
            let item = entry.item().identity().map_err(binding)?;
            let mut missing = false;
            let mut failed = false;
            for obligation in attempt
                .obligations
                .iter()
                .filter(|value| value.item == item)
            {
                match receipt_map.get(&(item, obligation.family)) {
                    Some(CandidateControlResultV1::Passed) => {}
                    Some(CandidateControlResultV1::Failed) => failed = true,
                    Some(CandidateControlResultV1::Unavailable) | None => missing = true,
                }
            }
            if failed {
                rejected.push(item);
            } else if missing {
                unresolved.push(item);
            } else {
                admitted.push(item);
            }
        }
        let status = if !rejected.is_empty() {
            CandidateClaimStatusV1::Rejected
        } else if unresolved.is_empty() {
            CandidateClaimStatusV1::Admitted
        } else {
            CandidateClaimStatusV1::Partial
        };
        claims.push(CandidateClaimOutcomeV1 {
            claim: claim.claim(),
            status,
            admitted_items: admitted,
            unresolved_items: unresolved,
            rejected_items: rejected,
        });
    }
    let outcome = CandidateAdmissionOutcomeV1 {
        schema_version: SCHEMA_V1,
        attempt: attempt.identity()?,
        evidence: evidence.identity()?,
        proposal: proposal.identity().map_err(binding)?,
        claims,
    };
    let _: ContentId<CandidateAdmissionOutcomeArtifact> = outcome.identity()?;
    Ok(outcome)
}

fn required_controls(plane: OraclePlaneV1) -> Vec<CandidateControlFamilyV1> {
    use CandidateControlFamilyV1 as C;
    use OraclePlaneV1 as P;
    match plane {
        P::ObservableSemantics
        | P::InputDomain
        | P::NumericalBehavior
        | P::ConcurrencyDeterminism => {
            vec![C::SourceBuild, C::ExecuteObservation, C::SemanticComparison]
        }
        P::InterfaceStructure | P::CoverageDiscovery => vec![
            C::SourceBuild,
            C::StaticAnalysis,
            C::ExecuteObservation,
            C::SemanticComparison,
        ],
        P::StateMemoryEffects | P::FailureRejection | P::CrossPlaneInteraction => vec![
            C::SourceBuild,
            C::StaticAnalysis,
            C::ExecuteObservation,
            C::SemanticComparison,
            C::Safety,
        ],
        P::ResourcePerformance => vec![C::SourceBuild, C::ExecuteObservation, C::Performance],
    }
}
fn obligation_key(value: &CandidateControlObligationV1) -> (String, CandidateControlFamilyV1) {
    (value.item.to_wire(), value.family)
}
fn receipt_key(value: &CandidateControlReceiptV1) -> (String, CandidateControlFamilyV1) {
    (value.item.to_wire(), value.family)
}
fn derive_id<T: Serialize, A: ContentType>(
    value: &T,
) -> Result<ContentId<A>, CandidateAdmissionError> {
    ContentId::derive(&cairn_codec::to_vec(value).map_err(codec)?).map_err(codec)
}
fn codec(error: impl std::fmt::Display) -> CandidateAdmissionError {
    CandidateAdmissionError::Codec(error.to_string())
}
fn binding(error: impl std::fmt::Display) -> CandidateAdmissionError {
    CandidateAdmissionError::Upstream(error.to_string())
}

#[derive(Debug, Error)]
pub enum CandidateAdmissionError {
    #[error("only Candidate Admission schema V1 is supported")]
    UnsupportedSchema,
    #[error("Candidate mechanism catalog changed")]
    MechanismCatalogDrift,
    #[error("Candidate Admission has no qualified {0:?} mechanism")]
    MissingMechanism(CandidateControlFamilyV1),
    #[error("Candidate Admission authority binding changed")]
    BindingMismatch,
    #[error("Candidate Admission attempt matrix changed")]
    AttemptDrift,
    #[error("Candidate Admission evidence changed")]
    EvidenceDrift,
    #[error("Candidate Admission outcome changed")]
    OutcomeDrift,
    #[error("upstream Candidate/Oracle artifact is invalid: {0}")]
    Upstream(String),
    #[error("Candidate Admission codec failed: {0}")]
    Codec(String),
}
