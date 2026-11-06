//! Tests of the built `mooring-portal` binary as an operator runs it.
//!
//! Every module is test code, including helpers that are not `#[test]`
//! functions; `#[cfg(test)]` declares that, so the workspace's test-only
//! allowances in `clippy.toml` cover them as they cover `#[test]` functions.

#[cfg(test)]
mod shutdown;
