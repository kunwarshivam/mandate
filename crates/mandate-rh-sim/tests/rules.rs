//! The place and review rules of the published contract, one example each (E7-25;
//! robinhood-contract.md, "The equity order tools").

mod common;

use common::{AGENTIC, NOT_AGENTIC, account, limit, sim};
use mandate_rh_sim::{OrderRequest, Sim, SimError};

type Outcome = Result<(), SimError>;

/// A two-share limit buy at 501 with these fields replaced; an empty value removes the field.
fn req(edits: &[(&str, &str)]) -> OrderRequest {
    let mut r = limit("buy", "2", "501", 1);
    for &(field, value) in edits {
        let v = (!value.is_empty()).then(|| value.to_owned());
        match field {
            "account" => r.account_number = value.to_owned(),
            "symbol" => r.symbol = value.to_owned(),
            "side" => r.side = value.to_owned(),
            "type" => r.order_type = value.to_owned(),
            "quantity" => r.quantity = v,
            "dollar_amount" => r.dollar_amount = v,
            "limit_price" => r.limit_price = v,
            "stop_price" => r.stop_price = v,
            "time_in_force" => r.time_in_force = v,
            "market_hours" => r.market_hours = v,
            _ => r.ref_id = v,
        }
    }
    r
}

#[test]
#[ignore = "pending E7-25"]
fn only_an_agentic_account_reviews_or_places() -> Outcome {
    let mut sim = sim()?;
    assert_eq!(
        sim.review(&req(&[("account", NOT_AGENTIC)])),
        Err(SimError::NotAgentic)
    );
    assert_eq!(
        sim.place(&req(&[("account", NOT_AGENTIC)])),
        Err(SimError::NotAgentic)
    );
    assert_eq!(
        sim.place(&req(&[("account", "5QR09999")])),
        Err(SimError::UnknownAccount)
    );
    assert_eq!(sim.orders(NOT_AGENTIC)?, vec![]);
    assert_eq!(sim.orders("5QR09999"), Err(SimError::UnknownAccount));
    let twice = vec![
        account(AGENTIC, true, false),
        account(AGENTIC, false, false),
    ];
    assert_eq!(Sim::new(twice).map(|_| ()), Err(SimError::DuplicateAccount));
    Ok(())
}

#[test]
fn each_refusal_names_what_it_refuses() {
    let stub = SimError::Unimplemented { story: "E7-25" };
    assert_eq!(stub.to_string(), "E7-25 has not been implemented yet");
    let field = SimError::Unreadable("ref_id").to_string();
    assert!(field.starts_with("`ref_id` is missing"), "{field}");
    let alert = SimError::Alert(mandate_rh_sim::Alert::Halt).to_string();
    assert_eq!(alert, "refused by a pre-trade alert: Halt");
}
