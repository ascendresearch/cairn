//! Task-generic reasoning decomposition selected for one migration run.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Frozen reasoning topology used by the SIR and Oracle proposal stages.
///
/// The variants change model decomposition and available evidence, never Intent or Oracle
/// Admission authority. They are production task policies so an ablation run uses the same public
/// workflow rather than a test-only bypass.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningDecompositionPolicyV1 {
    /// One SIR episode followed, after Intent Admission, by one whole-portfolio Oracle episode.
    MinimalDecomposition,
    /// Focused dimension/item development, Review, revision, and portfolio coherence episodes.
    StructuredReview,
    /// Structured Review plus Controller-authorized Worker experiments projected back as evidence.
    EvidenceAugmentedStructuredReview,
}

impl ReasoningDecompositionPolicyV1 {
    /// Whether the proposal topology uses focused discovery, Review, and revision Agent Loops.
    #[must_use]
    pub const fn uses_structured_review(self) -> bool {
        matches!(
            self,
            Self::StructuredReview | Self::EvidenceAugmentedStructuredReview
        )
    }

    /// Whether proposal Agent Loops may request new Controller-authorized Worker observations.
    #[must_use]
    pub const fn permits_worker_experiments(self) -> bool {
        matches!(self, Self::EvidenceAugmentedStructuredReview)
    }
}

impl fmt::Display for ReasoningDecompositionPolicyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MinimalDecomposition => "minimal-decomposition",
            Self::StructuredReview => "structured-review",
            Self::EvidenceAugmentedStructuredReview => "evidence-augmented-structured-review",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ReasoningDecompositionPolicyV1 as Policy;

    #[test]
    fn policies_preserve_the_two_ablation_factors() {
        assert!(!Policy::MinimalDecomposition.uses_structured_review());
        assert!(!Policy::MinimalDecomposition.permits_worker_experiments());
        assert!(Policy::StructuredReview.uses_structured_review());
        assert!(!Policy::StructuredReview.permits_worker_experiments());
        assert!(Policy::EvidenceAugmentedStructuredReview.uses_structured_review());
        assert!(Policy::EvidenceAugmentedStructuredReview.permits_worker_experiments());
    }

    #[test]
    fn policy_wire_names_are_current_v1_and_strict() {
        let encoded = cairn_codec::to_vec(&Policy::EvidenceAugmentedStructuredReview)
            .expect("policy encodes");
        assert_eq!(
            encoded,
            br#""evidence-augmented-structured-review""#.to_vec()
        );
        assert!(serde_json::from_slice::<Policy>(br#""unknown""#).is_err());
    }
}
