//! The agent bridge's data side: the jobs store, the ledger that turns jobs
//! into counted entries, and its accounting guardrails. Detection and the
//! supervising thread live in the app.

pub mod accounting;
pub mod ledger;
pub mod spans;
pub mod store;
