//! Tests of the token-exchange process, grouped by the module they test, with no network. A test
//! marked `#[ignore = "pending <story>"]` fails at that story's stub until the story's code lands.

mod form;
mod response;

#[cfg(debug_assertions)]
mod cell;
#[cfg(debug_assertions)]
mod http;
#[cfg(debug_assertions)]
mod startup;
