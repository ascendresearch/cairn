//! Names and permissions for the trees one Cairn deployment is laid out in.
//!
//! Runtime state is divided by ownership and mutability rather than by subject: what decides
//! permissions, backup policy and upgrade policy is who writes a thing, when it changes, and
//! whether losing it matters.
//!
//! This module gives those trees one home for their names and modes so that `bootstrap` creates
//! them and nothing else has to know them. It deliberately validates nothing at run time: the
//! layout is created by one command from this list, so a process checking the layout it was handed
//! would be checking what the same code just produced. What is worth testing is the end to end
//! claim, that a deployment created by `bootstrap` actually starts, and that is a test rather than
//! a gate.

use std::fmt;

use serde::{Deserialize, Serialize};

/// One tree in a deployment, distinguished by who owns it and what class of material it holds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeTree {
    /// Import source for knowledge and skill material. Never read at run time.
    Packs,
    /// Events, content-addressed storage and rebuildable derived indexes.
    Store,
    /// Admission-side hidden cases, expected values, private mutants and exposure ledgers.
    Restricted,
    /// PKI, enrollment material and credentials.
    Secrets,
    /// One workspace per migration project.
    Workspaces,
    /// Stable identities, counts, states and failure classes. Never diagnostic bodies.
    Log,
}

impl RuntimeTree {
    /// Every tree a controller deployment is laid out in.
    pub const CONTROLLER: &'static [Self] = &[
        Self::Packs,
        Self::Store,
        Self::Restricted,
        Self::Secrets,
        Self::Workspaces,
        Self::Log,
    ];

    /// Every tree a worker deployment is laid out in.
    ///
    /// A judged party does not share a host with its judge, so `packs/` and `restricted/` are
    /// absent here by not being created rather than by being refused somewhere.
    pub const WORKER: &'static [Self] = &[Self::Store, Self::Secrets, Self::Log];

    /// Returns the directory name this tree takes under a deployment root.
    #[must_use]
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Packs => "packs",
            Self::Store => "store",
            Self::Restricted => "restricted",
            Self::Secrets => "secrets",
            Self::Workspaces => "workspaces",
            Self::Log => "log",
        }
    }

    /// Returns the mode this tree is created with.
    ///
    /// The material class decides it. Secret and restricted material are readable only by the
    /// account that owns the deployment, which is what matters on a host shared with other people.
    #[must_use]
    pub const fn mode(self) -> u32 {
        match self {
            Self::Secrets | Self::Restricted => 0o700,
            Self::Packs | Self::Store | Self::Workspaces | Self::Log => 0o755,
        }
    }
}

impl fmt::Display for RuntimeTree {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.directory_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_deployment_carries_none_of_its_judge_s_trees() {
        for tree in [RuntimeTree::Packs, RuntimeTree::Restricted] {
            assert!(
                !RuntimeTree::WORKER.contains(&tree),
                "{tree} must not be laid out on a worker host"
            );
        }
    }

    #[test]
    fn material_class_decides_the_mode() {
        assert_eq!(RuntimeTree::Secrets.mode(), 0o700);
        assert_eq!(RuntimeTree::Restricted.mode(), 0o700);
        assert_eq!(RuntimeTree::Store.mode(), 0o755);
    }
}
