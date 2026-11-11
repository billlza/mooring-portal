//! Golden tests of the portal snapshot contract, version 1, against the
//! synthetic documents in `contract/fixtures/v1/`: `valid/` holds
//! documents an exporter may write, in the canonical pretty form;
//! `invalid/` one document per rule, each refused for that rule; `stored/`
//! the exact one-user documents the portal stores. The CFM profile bundle
//! contract, version 2, is tested the same way against
//! `contract/fixtures/cfm-profiles-v2/`, and what the portal reads of
//! GitHub's description of a CFM release against
//! `contract/fixtures/cfm-release/`. The payment contracts (the
//! catalogue, Stripe's events and the entitlements Mooring pulls) have
//! theirs in `payments`.
//!
//! Every module is test code, including helpers that are not `#[test]`
//! functions; `#[cfg(test)]` declares that, so the workspace's test-only
//! allowances in `clippy.toml` cover them as they cover `#[test]` functions.

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod payments;
#[cfg(test)]
mod profiles;
#[cfg(test)]
mod release;
