use cairn_protocol::ContentId;
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    OracleBuildTestSnapshotArtifact, OracleDocumentationSnapshotArtifact,
    OracleKnowledgeSnapshotArtifact,
};

const ORACLE_AGENT_TEXT_SNAPSHOT_LIMIT: usize = 512 * 1024;

macro_rules! oracle_agent_text_snapshot {
    ($name:ident, $wire:ident, $artifact:ty, $label:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        pub struct $name {
            identity: ContentId<$artifact>,
            text: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $wire {
            identity: ContentId<$artifact>,
            text: String,
        }

        impl $name {
            /// Reconstructs an identity-bound public Agent context snapshot.
            ///
            /// # Errors
            ///
            /// Rejects empty or oversized text and content-identity drift.
            pub fn new(
                identity: ContentId<$artifact>,
                text: String,
            ) -> Result<Self, OracleAgentContextError> {
                if text.is_empty()
                    || text.len() > ORACLE_AGENT_TEXT_SNAPSHOT_LIMIT
                    || ContentId::<$artifact>::derive(text.as_bytes())
                        .map_err(|error| OracleAgentContextError::Codec(error.to_string()))?
                        != identity
                {
                    return Err(OracleAgentContextError::InvalidSnapshot($label));
                }
                Ok(Self { identity, text })
            }

            #[must_use]
            pub fn text(&self) -> &str {
                &self.text
            }

            #[must_use]
            pub const fn identity(&self) -> ContentId<$artifact> {
                self.identity
            }
        }

        impl TryFrom<$wire> for $name {
            type Error = OracleAgentContextError;

            fn try_from(wire: $wire) -> Result<Self, Self::Error> {
                Self::new(wire.identity, wire.text)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                $wire::deserialize(deserializer)?
                    .try_into()
                    .map_err(de::Error::custom)
            }
        }
    };
}

oracle_agent_text_snapshot!(
    OracleAgentDocumentationV1,
    OracleAgentDocumentationWire,
    OracleDocumentationSnapshotArtifact,
    "Oracle documentation"
);
oracle_agent_text_snapshot!(
    OracleAgentBuildTestsV1,
    OracleAgentBuildTestsWire,
    OracleBuildTestSnapshotArtifact,
    "Oracle build/test"
);
oracle_agent_text_snapshot!(
    OracleAgentKnowledgeV1,
    OracleAgentKnowledgeWire,
    OracleKnowledgeSnapshotArtifact,
    "Oracle knowledge"
);

/// Exact product-owned public material exposed to an Oracle role hook.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleAgentMaterialsV1 {
    documentation: OracleAgentDocumentationV1,
    build_and_tests: OracleAgentBuildTestsV1,
    knowledge: OracleAgentKnowledgeV1,
}

impl OracleAgentMaterialsV1 {
    #[must_use]
    pub const fn new(
        documentation: OracleAgentDocumentationV1,
        build_and_tests: OracleAgentBuildTestsV1,
        knowledge: OracleAgentKnowledgeV1,
    ) -> Self {
        Self {
            documentation,
            build_and_tests,
            knowledge,
        }
    }

    #[must_use]
    pub const fn documentation(&self) -> &OracleAgentDocumentationV1 {
        &self.documentation
    }

    #[must_use]
    pub const fn build_and_tests(&self) -> &OracleAgentBuildTestsV1 {
        &self.build_and_tests
    }

    #[must_use]
    pub const fn knowledge(&self) -> &OracleAgentKnowledgeV1 {
        &self.knowledge
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum OracleAgentContextError {
    #[error("{0} Agent snapshot identity or size changed")]
    InvalidSnapshot(&'static str),
    #[error("Oracle Agent snapshot codec failed: {0}")]
    Codec(String),
}
