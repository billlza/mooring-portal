//! Golden tests of the portal snapshot contract, version 1, against the
//! synthetic documents in `contract/fixtures/v1/`: `valid/` holds
//! documents an exporter may write, in the canonical pretty form;
//! `invalid/` one document per rule, each refused for that rule; `stored/`
//! the exact one-user documents the portal stores. The CFM profile bundle
//! contract, version 1, is tested the same way against
//! `contract/fixtures/cfm-profiles-v1/`.
//!
//! Every module is test code, including helpers that are not `#[test]`
//! functions; `#[cfg(test)]` declares that, so the workspace's test-only
//! allowances in `clippy.toml` cover them as they cover `#[test]` functions.

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod profiles;
