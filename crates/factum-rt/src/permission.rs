//! Permission model — index-level access control.
//!
//! ## Critical Design Decision
//! Permission filtering MUST happen at the index layer, NOT as
//! post-query filtering. Post-query filtering leaks aggregate information:
//! if a query returns 10 results but 3 are filtered out, the caller
//! can infer that 3 confidential nodes exist even without seeing them.
//!
//! In v0.1, we filter at the candidate enumeration stage (functionally
//! equivalent to index-level). In production (RocksDB), this becomes
//! a bitmap intersection at the `by_perm` index.

use smol_str::SmolStr;
use factum_core::types::PermissionTag;

/// A principal's identity and roles.
#[derive(Clone, Debug)]
pub struct PermissionContext {
    /// Who is making the query.
    pub principal: SmolStr,
    /// Role bitmask. Must be a subset of PermissionTag bits.
    pub role_mask: u32,
}

impl Default for PermissionContext {
    fn default() -> Self {
        Self::public()
    }
}

impl PermissionContext {
    /// Create a context with public access only.
    pub fn public() -> Self {
        Self {
            principal: SmolStr::new("anonymous"),
            role_mask: PermissionTag::PUBLIC.0,
        }
    }

    /// Create a context with internal access.
    pub fn internal(principal: impl Into<SmolStr>) -> Self {
        Self {
            principal: principal.into(),
            role_mask: PermissionTag::PUBLIC.0 | PermissionTag::INTERNAL.0,
        }
    }

    /// Create a context with confidential access.
    pub fn confidential(principal: impl Into<SmolStr>) -> Self {
        Self {
            principal: principal.into(),
            role_mask: PermissionTag::PUBLIC.0 | PermissionTag::INTERNAL.0 | PermissionTag::CONFIDENTIAL.0,
        }
    }

    /// Create a context with full access (all roles).
    pub fn admin(principal: impl Into<SmolStr>) -> Self {
        Self {
            principal: principal.into(),
            role_mask: 0xFFFF_FFFF,
        }
    }

    /// Check if this context grants access to a permission tag.
    pub fn grants(&self, tag: PermissionTag) -> bool {
        tag.grants(self.role_mask)
    }

    /// Add a role to the mask.
    pub fn with_role(mut self, role: PermissionTag) -> Self {
        self.role_mask |= role.0;
        self
    }
}

/// Permission errors.
#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("permission denied: principal {principal} lacks required role")]
    Denied { principal: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_access() {
        let ctx = PermissionContext::public();
        assert!(ctx.grants(PermissionTag::PUBLIC));
        assert!(!ctx.grants(PermissionTag::INTERNAL));
        assert!(!ctx.grants(PermissionTag::CONFIDENTIAL));
    }

    #[test]
    fn test_internal_access() {
        let ctx = PermissionContext::internal("user1");
        assert!(ctx.grants(PermissionTag::PUBLIC));
        assert!(ctx.grants(PermissionTag::INTERNAL));
        assert!(!ctx.grants(PermissionTag::CONFIDENTIAL));
    }

    #[test]
    fn test_admin_access() {
        let ctx = PermissionContext::admin("root");
        assert!(ctx.grants(PermissionTag::PUBLIC));
        assert!(ctx.grants(PermissionTag::INTERNAL));
        assert!(ctx.grants(PermissionTag::CONFIDENTIAL));
        assert!(ctx.grants(PermissionTag::RESTRICTED));
    }

    #[test]
    fn test_role_addition() {
        let ctx = PermissionContext::public().with_role(PermissionTag::CONFIDENTIAL);
        assert!(ctx.grants(PermissionTag::PUBLIC));
        assert!(ctx.grants(PermissionTag::CONFIDENTIAL));
        assert!(!ctx.grants(PermissionTag::INTERNAL));
    }
}
