//! Project definitions and the shape of one migration workspace.
//!
//! A project definition declares which files the product provides and which the agent writes.
//! Narrowing the agent's writable surface constrains the action space, and more importantly it
//! makes a build failure attributable: when the scaffolding is not writable, a failed build is a
//! fact about the candidate rather than a question about who broke what.
//!
//! The upstream identity of the source is a gate. A definition that names an upstream nobody can
//! obtain cannot enter intake, and an upstream that does not pin exact bytes names no upstream at
//! all: a branch moves, so it identifies a policy for finding source rather than the source a
//! result was produced from.

use std::{
    collections::BTreeSet,
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// File name every project workspace carries at its root.
pub const DEFINITION_FILE: &str = "project.json";

/// A definition that cannot enter intake.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProjectError {
    /// Only the current definition schema is admitted.
    #[error("project definition schema_version {0} is unsupported")]
    UnsupportedSchema(u16),
    /// The project name cannot be a single directory segment.
    #[error("project name {0:?} is not a single path segment of permitted characters")]
    InvalidName(String),
    /// A declared file path is not usable inside a workspace.
    #[error("{path:?} is not a normalized workspace-relative file path")]
    InvalidPath {
        /// The path as declared.
        path: String,
    },
    /// One path is declared as both provided and agent-authored.
    #[error("{path:?} is declared both provided and authored by the agent")]
    AmbiguousOwnership {
        /// The path claimed by both sets.
        path: String,
    },
    /// The agent has no writable surface, so it cannot act on this project at all.
    #[error("a project definition must give the agent at least one file to author")]
    NoWritableSurface,
    /// The upstream does not name exact bytes.
    #[error("the source upstream does not pin an exact revision: {0}")]
    UnpinnedUpstream(String),
}

/// One directory segment naming a project inside the workspaces tree.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectName(String);

impl ProjectName {
    /// Creates a project name from one permitted path segment.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is not a single segment of lowercase alphanumerics,
    /// hyphens or underscores.
    pub fn new(value: impl Into<String>) -> Result<Self, ProjectError> {
        let value = value.into();
        let permitted = !value.is_empty()
            && value.len() <= 64
            && value.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || character == '-'
                    || character == '_'
            })
            && !value.starts_with('-');
        if permitted {
            Ok(Self(value))
        } else {
            Err(ProjectError::InvalidName(value))
        }
    }

    /// Returns the name as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProjectName {
    type Error = ProjectError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProjectName> for String {
    fn from(value: ProjectName) -> Self {
        value.0
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Where the frozen source came from.
///
/// Each variant names exact bytes. A form that does not is refused rather than accepted with a
/// warning, because a result produced from "whatever that branch pointed at" cannot be reproduced
/// and therefore cannot be evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum UpstreamIdentity {
    /// A git repository at one exact commit.
    Git {
        /// Repository locator, as an operator would clone it.
        repository: String,
        /// Full forty-character commit identity. An abbreviation is not admitted, because it names
        /// a prefix rather than a commit.
        commit: String,
    },
    /// Bytes already in the content-addressed store.
    Content {
        /// Content identity of the frozen source archive.
        content_id: String,
    },
}

impl UpstreamIdentity {
    fn validate(&self) -> Result<(), ProjectError> {
        match self {
            Self::Git { repository, commit } => {
                if repository.trim().is_empty() {
                    return Err(ProjectError::UnpinnedUpstream(
                        "the repository locator is empty".into(),
                    ));
                }
                if commit.len() != 40 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(ProjectError::UnpinnedUpstream(format!(
                        "{commit:?} is not a full commit identity"
                    )));
                }
                Ok(())
            }
            Self::Content { content_id } => {
                if content_id.trim().is_empty() {
                    return Err(ProjectError::UnpinnedUpstream(
                        "the content identity is empty".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// The upstream a project's frozen source is taken from.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDefinition {
    /// Exact upstream identity.
    pub upstream: UpstreamIdentity,
}

/// One project's intake declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectDefinition {
    /// Only version 1 is current.
    pub schema_version: u16,
    /// Stable project identity, which is also its directory in the workspaces tree.
    pub project: ProjectName,
    /// Where the frozen source comes from.
    pub source: SourceDefinition,
    /// Files the product supplies. The agent may read them and may not write them.
    pub provided: BTreeSet<String>,
    /// Files the agent writes. This is its entire writable surface.
    pub authored_by_agent: BTreeSet<String>,
}

impl ProjectDefinition {
    /// Checks every condition a definition must satisfy to enter intake.
    ///
    /// # Errors
    ///
    /// Returns the first condition the definition fails.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.schema_version != 1 {
            return Err(ProjectError::UnsupportedSchema(self.schema_version));
        }
        self.source.upstream.validate()?;
        for path in self.provided.iter().chain(&self.authored_by_agent) {
            if !is_workspace_file(path) {
                return Err(ProjectError::InvalidPath { path: path.clone() });
            }
        }
        if let Some(path) = self.provided.intersection(&self.authored_by_agent).next() {
            return Err(ProjectError::AmbiguousOwnership { path: path.clone() });
        }
        if self.authored_by_agent.is_empty() {
            return Err(ProjectError::NoWritableSurface);
        }
        Ok(())
    }

    /// Returns whether the agent may write this path.
    #[must_use]
    pub fn agent_may_write(&self, path: &str) -> bool {
        self.authored_by_agent.contains(path)
    }
}

/// Returns the workspace directory of one project inside the workspaces tree.
#[must_use]
pub fn project_directory(workspaces_root: &Path, project: &ProjectName) -> PathBuf {
    workspaces_root.join(project.as_str())
}

/// Returns the path of one project's definition inside the workspaces tree.
#[must_use]
pub fn definition_path(workspaces_root: &Path, project: &ProjectName) -> PathBuf {
    project_directory(workspaces_root, project).join(DEFINITION_FILE)
}

/// Accepts only a normalized relative file path that stays inside the workspace.
///
/// Two spellings of one file must not be able to land in different ownership sets, so a path is
/// admitted in exactly one form: relative, no empty or dot segments, no parent components, no
/// trailing separator, and no backslashes, which are a separator on some hosts and an ordinary
/// character on others.
fn is_workspace_file(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains('\\')
        && !path.contains("//")
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> ProjectDefinition {
        ProjectDefinition {
            schema_version: 1,
            project: ProjectName::new("reduce-sum-f32").expect("project name"),
            source: SourceDefinition {
                upstream: UpstreamIdentity::Git {
                    repository: "https://example.test/kernels.git".into(),
                    commit: "0123456789abcdef0123456789abcdef01234567".into(),
                },
            },
            provided: ["bin/run", "CMakeLists.txt"]
                .into_iter()
                .map(String::from)
                .collect(),
            authored_by_agent: ["source/kernel.cpp"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }

    #[test]
    fn a_complete_definition_enters_intake() {
        let definition = definition();
        definition.validate().expect("documented definition");
        assert!(definition.agent_may_write("source/kernel.cpp"));
        assert!(!definition.agent_may_write("bin/run"));
    }

    // A file claimed by both sets is the one case that defeats the purpose of having two sets: a
    // build failure could then be the candidate's fault or the scaffolding's, and nothing in the
    // record distinguishes them.
    #[test]
    fn a_file_cannot_be_both_provided_and_authored() {
        let mut definition = definition();
        definition.provided.insert("source/kernel.cpp".into());
        assert_eq!(
            definition.validate(),
            Err(ProjectError::AmbiguousOwnership {
                path: "source/kernel.cpp".into()
            })
        );
    }

    #[test]
    fn an_agent_with_nothing_to_author_cannot_enter_intake() {
        let mut definition = definition();
        definition.authored_by_agent.clear();
        assert_eq!(definition.validate(), Err(ProjectError::NoWritableSurface));
    }

    // An upstream that does not pin exact bytes names a way of finding source rather than the
    // source a result came from, so it is refused rather than accepted with a warning.
    #[test]
    fn an_upstream_that_does_not_pin_exact_bytes_is_refused() {
        for commit in [
            "main",
            "0123456",
            "0123456789abcdef0123456789abcdef0123456z",
        ] {
            let mut definition = definition();
            definition.source.upstream = UpstreamIdentity::Git {
                repository: "https://example.test/kernels.git".into(),
                commit: commit.into(),
            };
            assert!(
                matches!(
                    definition.validate(),
                    Err(ProjectError::UnpinnedUpstream(_))
                ),
                "{commit:?} must not be admitted as an upstream revision"
            );
        }
    }

    // Ownership is decided by comparing declared paths, so two spellings of one file would let it
    // sit in both sets without the disjointness check ever seeing a collision.
    #[test]
    fn only_one_spelling_of_a_path_is_admitted() {
        for path in [
            "/etc/passwd",
            "../outside",
            "source/../bin/run",
            "./source/kernel.cpp",
            "source//kernel.cpp",
            "source/",
            "source\\kernel.cpp",
            "",
        ] {
            let mut definition = definition();
            definition.provided.insert(path.into());
            assert!(
                matches!(definition.validate(), Err(ProjectError::InvalidPath { .. })),
                "{path:?} must not be admitted as a workspace file"
            );
        }
    }

    #[test]
    fn a_project_name_is_one_directory_segment() {
        for name in ["", "Upper", "with space", "a/b", "..", "-leading"] {
            assert!(
                ProjectName::new(name).is_err(),
                "{name:?} must not be a project name"
            );
        }
        assert_eq!(
            ProjectName::new("reduce-sum-f32").expect("name").as_str(),
            "reduce-sum-f32"
        );
    }

    #[test]
    fn a_definition_round_trips_through_its_document() {
        let definition = definition();
        let document = serde_json::to_string(&definition).expect("document");
        let decoded: ProjectDefinition = serde_json::from_str(&document).expect("decode");
        assert_eq!(decoded, definition);
        assert_eq!(
            definition_path(Path::new("/srv/cairn/workspaces"), &definition.project),
            Path::new("/srv/cairn/workspaces/reduce-sum-f32/project.json")
        );
    }
}
