//! Roles and permissions (docs/adr/0002-workspaces-and-roles.md). The permission matrix is
//! the one shown on the "Members & sharing" screen of the design.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A member's role in one project. Ordered: every role can do everything the roles
/// before it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ProjectRole {
    Viewer,
    Editor,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum WorkspaceRole {
    Member,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    ViewSheets,
    AskCopilot,
    EditSheets,
    ShareSheetLinks,
    /// The spec's "steward" capability, which the chosen role model folds into Admin.
    ApproveLogic,
    ManageSourcesAndMembers,
}

impl ProjectRole {
    pub const fn allows(self, permission: Permission) -> bool {
        let required = match permission {
            Permission::ViewSheets | Permission::AskCopilot => ProjectRole::Viewer,
            Permission::EditSheets | Permission::ShareSheetLinks => ProjectRole::Editor,
            Permission::ApproveLogic | Permission::ManageSourcesAndMembers => ProjectRole::Admin,
        };
        self as u8 >= required as u8
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            ProjectRole::Viewer => "viewer",
            ProjectRole::Editor => "editor",
            ProjectRole::Admin => "admin",
        }
    }
}

impl WorkspaceRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            WorkspaceRole::Member => "member",
            WorkspaceRole::Admin => "admin",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown role {0:?}")]
pub struct UnknownRole(pub String);

impl FromStr for ProjectRole {
    type Err = UnknownRole;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "viewer" => Ok(Self::Viewer),
            "editor" => Ok(Self::Editor),
            "admin" => Ok(Self::Admin),
            other => Err(UnknownRole(other.to_owned())),
        }
    }
}

impl FromStr for WorkspaceRole {
    type Err = UnknownRole;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "member" => Ok(Self::Member),
            "admin" => Ok(Self::Admin),
            other => Err(UnknownRole(other.to_owned())),
        }
    }
}

impl fmt::Display for ProjectRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Permission::*;
    use super::ProjectRole::*;
    use super::*;

    /// Mirrors the "What each role can do" table in the design.
    #[test]
    fn matrix_matches_design() {
        let table = [
            (ViewSheets, [true, true, true]),
            (AskCopilot, [true, true, true]),
            (EditSheets, [false, true, true]),
            (ShareSheetLinks, [false, true, true]),
            (ApproveLogic, [false, false, true]),
            (ManageSourcesAndMembers, [false, false, true]),
        ];
        for (permission, expected) in table {
            for (role, allowed) in [Viewer, Editor, Admin].into_iter().zip(expected) {
                assert_eq!(
                    role.allows(permission),
                    allowed,
                    "{role:?} / {permission:?}"
                );
            }
        }
    }

    #[test]
    fn string_round_trip() {
        for role in [Viewer, Editor, Admin] {
            assert_eq!(role.as_str().parse::<ProjectRole>().unwrap(), role);
        }
        assert!("owner".parse::<ProjectRole>().is_err());
    }
}
