//! What the `mandate-paper` binary prints after a run that refused nothing (E7-19 slice 5, E1a-bin,
//! the brief's founder steps 3 and 4, DEC-846 item 6): the order a dry run would place, each order
//! submitted, or the model's `Flat` or `Undecided`, one line each, ids and the signal only
//! (`AGENTS.md` rule 7). The expected lines are written out here, in the words `mandate-tracer`
//! prints, never derived from the outcome.

use mandate_accounting::{InstrumentId, Side};
use mandate_executor::{ClientOrderId, OrderType, Purpose, SubmitOrder, TimeInForce};
use mandate_modelhost::Signal;
use mandate_num::{Price, Qty};
use mandate_paper::{Outcome, PaperError, lines, stderr_line};
use mandate_shell::{Report, ShellError};

const ENTRY: &str = "md-01JPAPERENTRY0000000000001";

/// The order a dry run stops before: one share of SPY at a limit, in the regular session.
fn entry() -> SubmitOrder {
    SubmitOrder {
        client_order_id: ClientOrderId::parse(ENTRY).unwrap(),
        instrument: InstrumentId::new("b28f4066-5c6d-479b-a2af-85dc1a8f16fb").unwrap(),
        side: Side::Buy,
        qty: Qty::parse("1").unwrap(),
        order_type: OrderType::Limit,
        tif: TimeInForce::Day,
        limit_price: Some(Price::parse("612.34").unwrap()),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: Purpose::Open,
    }
}

fn cycle(report: Report) -> Outcome {
    Outcome::Cycle(Box::new(report))
}

/// Brief step 3: a dry run names the order it would place, by its id alone, and says nothing was
/// sent; an alert's key is not a line.
#[test]
fn a_dry_run_names_the_order_it_would_place() {
    let report = Report {
        submitted: Vec::new(),
        would_place: Some(entry()),
        alerts: vec!["gate.denied"],
    };
    let expected =
        ["would place md-01JPAPERENTRY0000000000001 (nothing sent; pass --place-one-order)"];
    assert_eq!(lines(&cycle(report)).unwrap(), expected);
}

/// Brief step 4: a placing run names each order it submitted, in order, by id alone.
#[test]
fn a_placing_run_names_each_submitted_order() {
    let report = Report {
        submitted: vec![ENTRY.to_owned(), "md-01JPAPERENTRY0000000000002".to_owned()],
        would_place: None,
        alerts: Vec::new(),
    };
    let expected = [
        "submitted md-01JPAPERENTRY0000000000001",
        "submitted md-01JPAPERENTRY0000000000002",
    ];
    assert_eq!(lines(&cycle(report)).unwrap(), expected);
}

/// FT-4 (DEC-157 item 4): a `Flat` or `Undecided` model sends nothing, and the line says which.
#[test]
fn no_output_names_the_signal_and_that_nothing_was_sent() {
    let flat = lines(&Outcome::NoOutput(Signal::Flat)).unwrap();
    assert_eq!(flat, ["the model output Flat; nothing sent"]);
    let undecided = lines(&Outcome::NoOutput(Signal::Undecided)).unwrap();
    assert_eq!(undecided, ["the model output Undecided; nothing sent"]);
}

/// DEC-877 item 1, rule 6: the cap's fail-closed stop prints its stable code alone on stderr, the
/// key-only alert DEC-858 item 5 asks for, and no message that could grow to name an order.
#[test]
fn the_caps_stop_prints_its_code_alone_on_stderr() {
    let stop = PaperError::Shell(ShellError::CancelUnconfirmed);
    assert_eq!(stderr_line(&stop), "cancel_unconfirmed");
}

/// DEC-846 item 6: every other stop still prints its message alone on stderr, a shell refusal
/// included, as the binary's tests pin for the refusals before the credentials.
#[test]
fn every_other_stop_prints_its_message_on_stderr() {
    let cases = [
        (PaperError::Control, "the control stream could not be read"),
        (
            PaperError::Shell(ShellError::SecondSubmission),
            "the tracer places one order per run, and the executor asked for another",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(stderr_line(&error), message);
    }
}
