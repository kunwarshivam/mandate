//! The runtime's view of an admitted mandate (DEC-953, DEC-970 item 1).

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_runtime::{ApprovalSettings, LiveScope, MandateView};
use mandate_spec::ValidatedMandate;

use crate::error::PortError;

/// The facts of a mandate view that the document does not carry, each its host's to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewFacts {
    /// The user ids the mandate's approver references resolve to among the workspace's members.
    pub approvers: BTreeSet<String>,
    pub author: String,
    pub restricted_instruments: BTreeSet<InstrumentId>,
    /// DEC-923's scope for a live `cli_confirm` grant; `None` refuses every one.
    pub live_cli_confirm: Option<LiveScope>,
}

/// The view the runtime admits `validated` as: its canonical version, its pinned instruments as
/// the working universe, and its approval settings over `facts`.
///
/// # Errors
/// [`PortError::Absent`] for a pinned instrument the runtime cannot name, or a document with no
/// canonical version.
pub fn runtime_view(
    validated: &ValidatedMandate,
    facts: ViewFacts,
) -> Result<MandateView, PortError> {
    let document = validated.mandate();
    let working_universe = document
        .universe
        .pinned_instruments
        .iter()
        .map(|instrument| {
            InstrumentId::new(instrument.asset_id.as_str()).map_err(|_| PortError::Absent {
                what: "a runtime instrument id",
            })
        })
        .collect::<Result<BTreeSet<_>, PortError>>()?;
    let environment = match document.environment {
        mandate_domain::Environment::Paper => mandate_journal::Environment::Paper,
        mandate_domain::Environment::Live => mandate_journal::Environment::Live,
    };
    Ok(MandateView {
        version: format!(
            "sha256:{}",
            document
                .version()
                .map_err(|_| PortError::Absent {
                    what: "the validated mandate's canonical version",
                })?
                .digest()
        ),
        working_universe,
        restricted_instruments: facts.restricted_instruments,
        approval: ApprovalSettings {
            approvers: facts.approvers,
            author: facts.author,
            timeout_s: i64::from(document.autonomy.approval.timeout_s),
            environment,
            live_cli_confirm: facts.live_cli_confirm,
        },
    })
}
