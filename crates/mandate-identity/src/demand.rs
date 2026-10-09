//! What a sensitive data API demands of its context (DEC-655, identity spec ID-8): not only a
//! workspace, but a context authorized for one [`Permission`]. A marker type names the permission,
//! and [`Permitted`] witnesses that a [`TenantContext`] was authorized for it. The only way to
//! obtain one is [`TenantContext::require`]; a data API that takes `&Permitted<'_, ReadRecords>`
//! cannot be handed a context authorized for another row of §4.2.
//!
//! A witness cannot be built, defaulted, or cloned outside the crate
//! (`permitted_cannot_be_constructed_outside_require`, DEC-655 item 5), and no other type is a
//! demanded permission:
//!
//! ```compile_fail,E0451
//! use mandate_identity::demand::{Permitted, ReadRecords};
//! fn forge(context: &mandate_identity::TenantContext) -> Permitted<'_, ReadRecords> {
//!     Permitted { context, demanded: std::marker::PhantomData }
//! }
//! ```
//!
//! ```compile_fail,E0599
//! use mandate_identity::demand::{Permitted, ReadRecords};
//! let _ = Permitted::<'static, ReadRecords>::default();
//! ```
//!
//! ```compile_fail,E0308
//! use mandate_identity::demand::{Permitted, ReadRecords};
//! fn copy<'a>(witness: &Permitted<'a, ReadRecords>) -> Permitted<'a, ReadRecords> {
//!     witness.clone()
//! }
//! ```
//!
//! ```compile_fail,E0277
//! use mandate_identity::{demand::RequiredPermission, Permission};
//! enum Forged {}
//! impl RequiredPermission for Forged {
//!     const PERMISSION: Permission = Permission::ViewAgents;
//! }
//! ```
//!
//! A data API that demands the witness rejects a bare context:
//!
//! ```compile_fail,E0308
//! use mandate_identity::{demand::{Permitted, ReadRecords}, TenantContext};
//! fn read_records(_: &Permitted<'_, ReadRecords>) {}
//! fn route(context: &TenantContext) {
//!     read_records(context)
//! }
//! ```
//!
//! The control: it takes the witness `require` yields, which is also a [`Tenant`].
//!
//! ```
//! use mandate_identity::{
//!     demand::{Permitted, ReadRecords},
//!     Refusal, Tenant, TenantContext,
//! };
//! fn read_records(_: &Permitted<'_, ReadRecords>) {}
//! fn data_api(_: &impl Tenant) {}
//! fn route(context: &TenantContext) -> Result<(), Refusal> {
//!     let witness = context.require::<ReadRecords>()?;
//!     read_records(&witness);
//!     data_api(&witness);
//!     Ok(())
//! }
//! ```

use std::marker::PhantomData;

#[cfg(doc)]
use crate::Tenant;
use crate::{Permission, TenantContext};

/// A permission a data API can demand. It is sealed: only this module's markers implement it, one
/// for each permission a data API demands, added only when one does (DEC-655 item 1).
pub trait RequiredPermission: sealed::Sealed {
    /// The §4.2 row the marker names.
    const PERMISSION: Permission;
}

/// The marker for [`Permission::ReadRecords`]: reading records and verification, and export
/// (journal spec §7). It has no values; it is only a type parameter of [`Permitted`].
#[derive(Debug)]
pub enum ReadRecords {}

impl RequiredPermission for ReadRecords {
    const PERMISSION: Permission = Permission::ReadRecords;
}

/// A witness that a [`TenantContext`] was authorized for `P`'s permission (DEC-655 item 1). It
/// borrows the context and is a [`Tenant`] for it. It has private fields, no `Clone`, no `Default`,
/// and no constructor but [`TenantContext::require`].
#[derive(Debug)]
pub struct Permitted<'a, P: RequiredPermission> {
    pub(crate) context: &'a TenantContext,
    pub(crate) demanded: PhantomData<P>,
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::ReadRecords {}
}
