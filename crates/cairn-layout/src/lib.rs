//! Runtime tree layout for Cairn deployments.
//!
//! Runtime state is divided by ownership and mutability rather than by subject: what decides
//! permissions, backup policy and upgrade policy is who writes a thing, when it changes, and
//! whether losing it matters. Each tree is configured independently and resolves to an absolute
//! path, so a single-root deployment and a system installation are two bindings of one set of
//! logical roles rather than two layouts.
//!
//! Two properties here are the reason this is a module and not a convention. A process may only
//! name the trees its role owns, so a worker cannot obtain a path under `packs/` or `restricted/`
//! even by mistake. And no tree may be nested inside another, which is what makes the separation
//! between restricted material and secret material a fact about the filesystem instead of a rule
//! someone has to remember.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Environment variable naming the single root a bundled deployment resolves its trees under.
pub const HOME_VARIABLE: &str = "CAIRN_HOME";

/// One runtime tree, distinguished by who owns it and what class of material it holds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeTree {
    /// Administrator-owned process configuration, runtime models, target context and policy.
    Config,
    /// Import source for knowledge and skill material. Never read at runtime.
    Packs,
    /// Controller-owned events, content-addressed storage and rebuildable derived indexes.
    Store,
    /// Admission-side hidden cases, expected values, private mutants and exposure ledgers.
    Restricted,
    /// Administrator-owned PKI, enrollment material and credentials.
    Secrets,
    /// One workspace per migration project.
    Workspaces,
    /// Stable identities, counts, states and failure classes. Never diagnostic bodies.
    Log,
}

impl RuntimeTree {
    /// Returns the directory name this tree takes under a single deployment root.
    #[must_use]
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Packs => "packs",
            Self::Store => "store",
            Self::Restricted => "restricted",
            Self::Secrets => "secrets",
            Self::Workspaces => "workspaces",
            Self::Log => "log",
        }
    }
}

impl fmt::Display for RuntimeTree {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.directory_name())
    }
}

/// The part a process plays, which fixes the trees it may name.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutRole {
    /// Owns every tree.
    Controller,
    /// Owns only what it needs to run work. A judged party does not share a host with its judge,
    /// so `packs/` and `restricted/` are absent here by construction rather than by policy.
    Worker,
}

impl LayoutRole {
    /// Returns the trees this role owns, in a stable order.
    #[must_use]
    pub const fn trees(self) -> &'static [RuntimeTree] {
        match self {
            Self::Controller => &[
                RuntimeTree::Config,
                RuntimeTree::Packs,
                RuntimeTree::Store,
                RuntimeTree::Restricted,
                RuntimeTree::Secrets,
                RuntimeTree::Workspaces,
                RuntimeTree::Log,
            ],
            Self::Worker => &[
                RuntimeTree::Config,
                RuntimeTree::Store,
                RuntimeTree::Secrets,
                RuntimeTree::Log,
            ],
        }
    }

    /// Returns whether this role owns the given tree.
    #[must_use]
    pub fn owns(self, tree: RuntimeTree) -> bool {
        self.trees().contains(&tree)
    }
}

/// A layout that could not be resolved into separated absolute trees.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LayoutError {
    /// No root was configured for a tree the role requires.
    #[error("no root is configured for the {tree} tree, and {home} is unset", home = HOME_VARIABLE)]
    UnresolvedTree {
        /// The tree that has no root.
        tree: RuntimeTree,
    },
    /// A root was configured for a tree the role does not own.
    #[error("the {role:?} role does not own the {tree} tree")]
    ForeignTree {
        /// The role that was asked for it.
        role: LayoutRole,
        /// The tree it does not own.
        tree: RuntimeTree,
    },
    /// A configured path is not usable as a root.
    #[error("the {tree} root {path} is not an absolute path without parent components")]
    UnusableRoot {
        /// The tree whose root is unusable.
        tree: RuntimeTree,
        /// The path as configured.
        path: PathBuf,
    },
    /// One tree lies inside another, which voids the separation between their material classes.
    #[error("the {inner} tree at {inner_path} lies inside the {outer} tree at {outer_path}")]
    NestedTrees {
        /// The tree that lies inside the other.
        inner: RuntimeTree,
        /// Its root.
        inner_path: PathBuf,
        /// The tree that contains it.
        outer: RuntimeTree,
        /// Its root.
        outer_path: PathBuf,
    },
    /// A path within a tree would leave that tree.
    #[error("{path} does not stay within the {tree} tree")]
    EscapesTree {
        /// The tree the path was resolved against.
        tree: RuntimeTree,
        /// The path as given.
        path: PathBuf,
    },
}

/// Explicit per-tree roots, overriding what a single deployment root would give.
pub type TreeRoots = BTreeMap<RuntimeTree, PathBuf>;

/// Declares where a deployment's runtime trees live.
///
/// A bundled deployment names one root and takes the conventional directory under it for each
/// tree; a system installation names each root. Both are bindings of the same logical roles, which
/// is why one type expresses both rather than two configuration shapes existing side by side.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutConfig {
    /// Single deployment root. When absent, `CAIRN_HOME` supplies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<PathBuf>,
    /// Explicit roots, overriding what `home` would give for those trees.
    #[serde(default, skip_serializing_if = "TreeRoots::is_empty")]
    pub roots: TreeRoots,
}

/// Absolute, mutually disjoint roots for the trees one process owns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeLayout {
    role: LayoutRole,
    roots: BTreeMap<RuntimeTree, PathBuf>,
}

impl RuntimeLayout {
    /// Resolves the trees a role owns from an optional deployment root plus explicit overrides.
    ///
    /// A system installation supplies every root explicitly and no home; a bundled deployment
    /// supplies a home and no overrides. Both produce the same shape, which is the point: there is
    /// one layout with two bindings rather than two layouts.
    ///
    /// # Errors
    ///
    /// Returns an error when a required tree has no root, when a root is configured for a tree the
    /// role does not own, when a root is not an absolute path free of parent components, or when
    /// one resolved root lies inside another.
    pub fn resolve(
        role: LayoutRole,
        home: Option<&Path>,
        overrides: &TreeRoots,
    ) -> Result<Self, LayoutError> {
        for tree in overrides.keys() {
            if !role.owns(*tree) {
                return Err(LayoutError::ForeignTree { role, tree: *tree });
            }
        }
        let mut roots = BTreeMap::new();
        for tree in role.trees() {
            let configured = overrides
                .get(tree)
                .cloned()
                .or_else(|| home.map(|home| home.join(tree.directory_name())))
                .ok_or(LayoutError::UnresolvedTree { tree: *tree })?;
            if !usable_root(&configured) {
                return Err(LayoutError::UnusableRoot {
                    tree: *tree,
                    path: configured,
                });
            }
            roots.insert(*tree, configured);
        }
        ensure_disjoint(&roots)?;
        Ok(Self { role, roots })
    }

    /// Returns the role whose trees this layout carries.
    #[must_use]
    pub const fn role(&self) -> LayoutRole {
        self.role
    }

    /// Returns the absolute root of one tree.
    ///
    /// # Errors
    ///
    /// Returns an error when this layout's role does not own the tree.
    pub fn root(&self, tree: RuntimeTree) -> Result<&Path, LayoutError> {
        self.roots
            .get(&tree)
            .map(PathBuf::as_path)
            .ok_or(LayoutError::ForeignTree {
                role: self.role,
                tree,
            })
    }

    /// Resolves one tree-relative path into an absolute path inside that tree.
    ///
    /// The relative path may not be absolute and may not contain a parent component, so a
    /// configuration cannot reach out of the tree that owns the material it names.
    ///
    /// # Errors
    ///
    /// Returns an error when the role does not own the tree, or when the path would leave it.
    pub fn resolve_in(
        &self,
        tree: RuntimeTree,
        relative: impl AsRef<Path>,
    ) -> Result<PathBuf, LayoutError> {
        let relative = relative.as_ref();
        let root = self.root(tree)?;
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            return Err(LayoutError::EscapesTree {
                tree,
                path: relative.to_path_buf(),
            });
        }
        Ok(root.join(relative))
    }
}

fn usable_root(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

/// Rejects any layout where one tree lies inside another.
///
/// Restricted material and secret material are different classes with different permissions,
/// backup periods and access subjects. Nesting one tree inside another silently gives the outer
/// tree's permissions to the inner one's material, which is exactly the arrangement this check
/// exists to make impossible rather than merely discouraged.
fn ensure_disjoint(roots: &BTreeMap<RuntimeTree, PathBuf>) -> Result<(), LayoutError> {
    for (outer, outer_path) in roots {
        for (inner, inner_path) in roots {
            if outer == inner {
                continue;
            }
            if inner_path.starts_with(outer_path) {
                return Err(LayoutError::NestedTrees {
                    inner: *inner,
                    inner_path: inner_path.clone(),
                    outer: *outer,
                    outer_path: outer_path.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/srv/cairn")
    }

    #[test]
    fn a_bundled_deployment_resolves_every_tree_its_role_owns_under_one_root() {
        let layout =
            RuntimeLayout::resolve(LayoutRole::Controller, Some(&home()), &TreeRoots::new())
                .expect("controller layout");
        assert_eq!(
            layout.root(RuntimeTree::Store).expect("store"),
            Path::new("/srv/cairn/store")
        );
        assert_eq!(
            layout.root(RuntimeTree::Secrets).expect("secrets"),
            Path::new("/srv/cairn/secrets")
        );
        assert_eq!(
            layout.root(RuntimeTree::Restricted).expect("restricted"),
            Path::new("/srv/cairn/restricted")
        );
    }

    // A system installation names every root and has no deployment root at all. It has to produce
    // the same shape as a bundled deployment, because the trees are logical roles and a packaging
    // choice is not allowed to become a second layout.
    #[test]
    fn a_system_installation_needs_no_deployment_root() {
        let mut roots = TreeRoots::new();
        for tree in LayoutRole::Worker.trees() {
            roots.insert(
                *tree,
                PathBuf::from("/var/lib/cairn").join(tree.directory_name()),
            );
        }
        let layout = RuntimeLayout::resolve(LayoutRole::Worker, None, &roots).expect("worker");
        assert_eq!(
            layout.root(RuntimeTree::Store).expect("store"),
            Path::new("/var/lib/cairn/store")
        );
    }

    // The judged party does not share a host with its judge. That is meant to be a fact about the
    // filesystem, so a worker must not be able to name those trees even by mistake, and configuring
    // one has to fail rather than be quietly ignored.
    #[test]
    fn a_worker_cannot_name_the_trees_that_belong_to_its_judge() {
        let layout = RuntimeLayout::resolve(LayoutRole::Worker, Some(&home()), &TreeRoots::new())
            .expect("worker layout");
        for tree in [
            RuntimeTree::Packs,
            RuntimeTree::Restricted,
            RuntimeTree::Workspaces,
        ] {
            assert_eq!(
                layout.root(tree),
                Err(LayoutError::ForeignTree {
                    role: LayoutRole::Worker,
                    tree
                })
            );
        }
        let mut roots = TreeRoots::new();
        roots.insert(
            RuntimeTree::Restricted,
            PathBuf::from("/srv/cairn/restricted"),
        );
        assert_eq!(
            RuntimeLayout::resolve(LayoutRole::Worker, Some(&home()), &roots),
            Err(LayoutError::ForeignTree {
                role: LayoutRole::Worker,
                tree: RuntimeTree::Restricted
            })
        );
    }

    // The arrangement this rejects is the one the deployment actually had: durable state living
    // under the secret tree, where it inherits permissions, a backup period and an access subject
    // that belong to different material.
    #[test]
    fn durable_state_may_not_live_inside_the_secret_tree() {
        let mut roots = TreeRoots::new();
        for tree in LayoutRole::Controller.trees() {
            roots.insert(*tree, home().join(tree.directory_name()));
        }
        roots.insert(
            RuntimeTree::Store,
            PathBuf::from("/srv/cairn/secrets/state"),
        );
        let error = RuntimeLayout::resolve(LayoutRole::Controller, None, &roots)
            .expect_err("a store inside the secret tree must not resolve");
        assert!(matches!(
            error,
            LayoutError::NestedTrees {
                inner: RuntimeTree::Store,
                outer: RuntimeTree::Secrets,
                ..
            }
        ));
    }

    // Containment is a question about path components, not about string prefixes. Two trees whose
    // names share a prefix are siblings, and rejecting them would make ordinary layouts
    // unconfigurable for no reason.
    #[test]
    fn trees_whose_names_share_a_prefix_are_siblings_rather_than_nested() {
        let mut roots = TreeRoots::new();
        for tree in LayoutRole::Worker.trees() {
            roots.insert(*tree, PathBuf::from("/srv").join(tree.directory_name()));
        }
        roots.insert(RuntimeTree::Store, PathBuf::from("/srv/store"));
        roots.insert(RuntimeTree::Secrets, PathBuf::from("/srv/store-secrets"));
        let layout = RuntimeLayout::resolve(LayoutRole::Worker, None, &roots)
            .expect("sibling roots that share a name prefix must resolve");
        assert_eq!(
            layout.root(RuntimeTree::Secrets).expect("secrets"),
            Path::new("/srv/store-secrets")
        );
    }

    #[test]
    fn a_root_must_be_absolute_and_free_of_parent_components() {
        for candidate in ["relative/store", "/srv/cairn/../elsewhere"] {
            let mut roots = TreeRoots::new();
            for tree in LayoutRole::Worker.trees() {
                roots.insert(*tree, home().join(tree.directory_name()));
            }
            roots.insert(RuntimeTree::Store, PathBuf::from(candidate));
            assert!(
                matches!(
                    RuntimeLayout::resolve(LayoutRole::Worker, None, &roots),
                    Err(LayoutError::UnusableRoot {
                        tree: RuntimeTree::Store,
                        ..
                    })
                ),
                "{candidate} must not be usable as a root"
            );
        }
    }

    #[test]
    fn a_path_inside_a_tree_may_not_leave_it() {
        let layout = RuntimeLayout::resolve(LayoutRole::Worker, Some(&home()), &TreeRoots::new())
            .expect("worker layout");
        assert_eq!(
            layout
                .resolve_in(RuntimeTree::Store, "journal.sqlite3")
                .expect("inside"),
            Path::new("/srv/cairn/store/journal.sqlite3")
        );
        for escape in ["../secrets/ca.pem", "/etc/passwd"] {
            assert!(
                matches!(
                    layout.resolve_in(RuntimeTree::Store, escape),
                    Err(LayoutError::EscapesTree {
                        tree: RuntimeTree::Store,
                        ..
                    })
                ),
                "{escape} must not resolve inside the store tree"
            );
        }
    }

    #[test]
    fn a_missing_root_is_reported_rather_than_guessed() {
        let mut roots = TreeRoots::new();
        roots.insert(RuntimeTree::Store, home().join("store"));
        assert!(matches!(
            RuntimeLayout::resolve(LayoutRole::Worker, None, &roots),
            Err(LayoutError::UnresolvedTree { .. })
        ));
    }
}
