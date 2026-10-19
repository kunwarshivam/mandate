//! `cli_confirm` step-up for a paper grant made in the web app
//! ([DEC-2104](../../../docs/project/decisions/DEC-2104.md), DEC-2103 item 7, decided by the founder
//! in DEC-2605 row 5): the code the web app shows for a challenge, and the port that verifies a
//! grant's `cli_confirm` evidence against the challenge this server holds and the owner's
//! `StepUpConfirmed` on the control stream ([journal spec](../../../docs/specs/journal.md) §9.29).
//!
//! The owner confirms on the host, where only the CLI appends to the control stream, so a
//! confirmation is one the browser cannot make (DEC-2104 item 1). A confirmation is not step-up
//! evidence: only this port reads it, and the grant's own evidence is still judged by the runtime
//! as mandate spec §6.1 says.

use std::sync::Arc;

use mandate_api::envelope::StepUpEvidence;
use mandate_api::problem::ProblemCode;

use crate::challenges::Challenges;
use crate::stepup::{Action, Judged, StepUp};
use crate::subject::NamesPrincipal;

/// How many characters the code has: 40 bits of Crockford base 32 (DEC-2104 item 1).
pub const CODE_LEN: usize = 8;

/// The code the web app shows for a challenge whose `webauthn_challenge` is `bytes`: their first
/// 40 bits as [`CODE_LEN`] Crockford base-32 characters, upper case, without the display hyphen.
/// Fewer than 5 bytes has no code.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn code(_bytes: &[u8]) -> Option<String> {
    todo!()
}

/// The port for `cli_confirm` evidence (DEC-2104 item 6), over the challenges this server holds
/// and the subject links that must still name the confirming principal.
pub struct CliConfirm {
    pub challenges: Arc<Challenges>,
    pub links: Arc<dyn NamesPrincipal>,
}

impl StepUp for CliConfirm {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn verify(
        &self,
        _evidence: &StepUpEvidence,
        _action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        todo!()
    }
}
