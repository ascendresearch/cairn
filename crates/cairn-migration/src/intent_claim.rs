//! Structured task-authority semantics promoted by independent Intent Admission.

use serde::{Deserialize, Serialize};

use crate::SirCallerClaimId;

/// Membership requirement for collection-like outputs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionMembershipContractV1 {
    ExactSelectedOccurrences,
}

/// Reported cardinality requirement for collection-like outputs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionReportedCountContractV1 {
    ExactSelectedOccurrenceCount,
}

/// Whether sequence position is part of a collection-output contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOutputOrderContractV1 {
    UnspecifiedPermutation,
    StableInputRelative,
}

/// Machine-readable desired semantics supplied by the actual task authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionOutputIntentV1 {
    selection_claim: SirCallerClaimId,
    membership: CollectionMembershipContractV1,
    reported_count: CollectionReportedCountContractV1,
    order: CollectionOutputOrderContractV1,
}

impl CollectionOutputIntentV1 {
    #[must_use]
    pub const fn exact_selected_occurrences(
        selection_claim: SirCallerClaimId,
        order: CollectionOutputOrderContractV1,
    ) -> Self {
        Self {
            selection_claim,
            membership: CollectionMembershipContractV1::ExactSelectedOccurrences,
            reported_count: CollectionReportedCountContractV1::ExactSelectedOccurrenceCount,
            order,
        }
    }

    #[must_use]
    pub const fn selection_claim(&self) -> &SirCallerClaimId {
        &self.selection_claim
    }

    #[must_use]
    pub const fn order(&self) -> CollectionOutputOrderContractV1 {
        self.order
    }
}

/// Current structured claim families that an actual task authority may state directly.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "contract", rename_all = "kebab-case")]
pub enum AuthoritativeIntentClaimV1 {
    CollectionOutput(CollectionOutputIntentV1),
}
