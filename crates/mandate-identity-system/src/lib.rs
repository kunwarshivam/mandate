#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The context of the deployment's own background processes ([identity spec](../../../docs/specs/identity.md)
//! §4.5, ID-1, ID-5, DEC-642 items 5 to 8, DEC-668): one workspace and the process's workload
//! identity, recorded as `actor.kind` `agent` for an agent's runtime and `system` for the executor
//! and the scheduler. It is not an output of `authorize`, and the request path never holds one:
//! this crate's `allowed_dependents` in `xtask/layers.toml` admit only the bootstrap crates of the
//! agent runtime, the executor, and the scheduler. It never gates risk reduction (`AGENTS.md`
//! rule 13).
//!
//! It cannot be built but by [`SystemContext::new`], defaulted, or cloned:
//!
//! ```compile_fail,E0451
//! use mandate_identity::{OrgId, PrincipalId, WorkspaceId};
//! use mandate_identity_system::{Registration, SystemActor, SystemContext};
//! let registration = Registration {
//!     workload: PrincipalId(3),
//!     actor: SystemActor::System,
//!     permissions: Default::default(),
//! };
//! let _ = SystemContext { org: OrgId(1), workspace: WorkspaceId(2), registration };
//! ```
//!
//! ```compile_fail,E0599
//! let _ = mandate_identity_system::SystemContext::default();
//! ```
//!
//! ```compile_fail,E0308
//! use mandate_identity_system::SystemContext;
//! fn copy(context: &SystemContext) -> SystemContext {
//!     context.clone()
//! }
//! ```
//!
//! Its witness comes only from [`SystemContext::require`], never from a literal, and no other
//! type grants a permission:
//!
//! ```compile_fail,E0451
//! use mandate_identity::demand::{Permitted, ReadRecords};
//! fn forge(context: &mandate_identity_system::SystemContext) -> Permitted<'_, ReadRecords> {
//!     Permitted { context, demanded: std::marker::PhantomData }
//! }
//! ```
//!
//! ```compile_fail,E0277
//! use mandate_identity::{demand::Grants, Permission};
//! #[derive(Debug)]
//! struct Forged;
//! impl Grants for Forged {
//!     fn grants(&self, _: Permission) -> bool { true }
//! }
//! ```
//!
//! The control: a sensitive data API takes the witness `require` yields.
//!
//! ```
//! use mandate_identity::{demand::{Permitted, ReadRecords}, Refusal, Tenant};
//! use mandate_identity_system::SystemContext;
//! fn read_records(_: &Permitted<'_, ReadRecords>) {}
//! fn data_api(_: &impl Tenant) {}
//! fn job(context: &SystemContext) -> Result<(), Refusal> {
//!     data_api(context);
//!     read_records(&context.require::<ReadRecords>()?);
//!     Ok(())
//! }
//! ```

use std::collections::BTreeSet;

use mandate_identity::demand::{Grants, Permitted, RequiredPermission};
use mandate_identity::{
    OrgId, Permission, PrincipalId, PrincipalKind, Refusal, Tenant, WorkspaceId,
};

/// Who a background process is (DEC-668 item 3): no other actor kind is representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemActor {
    /// An agent's runtime, recorded as `actor.kind` `agent`.
    Agent,
    /// The executor or the scheduler, recorded as `actor.kind` `system`.
    System,
}

/// What a background process is registered as (DEC-668 item 3): its workload identity (identity
/// spec §3.1), its actor, and the permissions it lists. The bootstrap crates list none of the
/// sensitive ones until a story needs one (DEC-655 item 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The process's workload identity.
    pub workload: PrincipalId,
    /// Its actor kind.
    pub actor: SystemActor,
    /// The permissions it may demand a witness for, and no other.
    pub permissions: BTreeSet<Permission>,
}

/// A background process's context for one workspace (DEC-642 item 5). Private fields, no
/// `Default`, no `Clone`; its only constructor is [`SystemContext::new`].
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "its fields are read by the accessors E9-8 implements, stubs until then"
)]
pub struct SystemContext {
    org: OrgId,
    workspace: WorkspaceId,
    registration: Registration,
}

impl SystemContext {
    /// The context of a process registered as `registration`, for `workspace` of `org`, both read
    /// by the bootstrap from the workspace's own record.
    pub fn new(org: OrgId, workspace: WorkspaceId, registration: Registration) -> Self {
        Self {
            org,
            workspace,
            registration,
        }
    }
}

#[expect(
    clippy::todo,
    reason = "E9-8's stubs: `require` returns a `Refusal`, which has no `Unimplemented`, and the \
              accessors return plain values, so each is todo!(), the other form DEC-137 names"
)]
impl SystemContext {
    /// The actor it was registered as.
    pub fn actor(&self) -> SystemActor {
        todo!()
    }

    /// The witness a sensitive data API demands (DEC-655 item 4): `Ok` exactly when the
    /// registration lists `P`'s permission, and otherwise [`Refusal::Forbidden`].
    pub fn require<P: RequiredPermission>(&self) -> Result<Permitted<'_, P>, Refusal> {
        todo!()
    }
}

impl mandate_tenant::Sealed for SystemContext {}

#[expect(clippy::todo, reason = "E9-8's stubs, todo!() as DEC-137 names")]
impl Tenant for SystemContext {
    fn workspace(&self) -> WorkspaceId {
        todo!()
    }
    fn org(&self) -> OrgId {
        todo!()
    }
    fn principal(&self) -> PrincipalId {
        todo!()
    }
    fn kind(&self) -> PrincipalKind {
        todo!()
    }
}

#[expect(clippy::todo, reason = "E9-8's stub, todo!() as DEC-137 names")]
impl Grants for SystemContext {
    fn grants(&self, _permission: Permission) -> bool {
        todo!()
    }
}
