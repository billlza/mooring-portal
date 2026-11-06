//! Reading the synthetic documents in `contract/fixtures/v1/`.

use std::fs;
use std::path::PathBuf;

use portal_core::snapshot::{MooringUserId, Snapshot};

/// The directory of the version-1 fixtures.
pub(crate) fn fixture_dir(sub: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "../../contract/fixtures/v1",
        sub,
    ]
    .iter()
    .collect()
}

pub(crate) fn fixture(path: &str) -> String {
    let file: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../contract/fixtures/v1",
        path,
    ]
    .iter()
    .collect();
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

pub(crate) fn pilot() -> Snapshot {
    Snapshot::parse(fixture("valid/pilot.json").as_bytes()).unwrap()
}

pub(crate) fn id(text: &str) -> MooringUserId {
    MooringUserId::parse(text).unwrap()
}
