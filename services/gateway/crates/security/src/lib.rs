//! Authentication primitives, role-based access control, identity persistence and the
//! audit log. Later phases add credential encryption and row-level security here.

pub mod audit;
pub mod identity;
pub mod password;
pub mod rbac;
pub mod token;

pub use identity::{IdentityError, Principal, SessionStore};
pub use rbac::{Permission, ProjectRole, WorkspaceRole};
pub use token::SessionToken;
