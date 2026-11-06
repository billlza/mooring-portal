//! The CFM releases the download page offers.
//!
//! Only published releases are listed, checked against
//! `billlza/cfw-rs` on GitHub on 2026-11-06: v0.5.0 is the latest
//! published release (0.4.0 is an unpublished draft), and no Windows build
//! has been published. A new release updates this constant.

/// A published macOS release of CFM.
#[derive(Debug)]
pub(crate) struct MacRelease {
    /// The version.
    pub(crate) version: &'static str,
    /// The UTC day it was published.
    pub(crate) published: &'static str,
    /// The release's page.
    pub(crate) page_url: &'static str,
    /// The disk image's file name.
    pub(crate) dmg_name: &'static str,
    /// The disk image's download address.
    pub(crate) dmg_url: &'static str,
    /// The disk image's SHA-256, as GitHub and the release's
    /// `distribution-set.seal.json` list it.
    pub(crate) dmg_sha256: &'static str,
}

/// CFM 0.5.0, the first release with the mainland China direct switch.
pub(crate) const MAC: MacRelease = MacRelease {
    version: "0.5.0",
    published: "2026-10-18",
    page_url: "https://github.com/billlza/cfw-rs/releases/tag/v0.5.0",
    dmg_name: "Clash.for.Mac_0.5.0_arm64.dmg",
    dmg_url: "https://github.com/billlza/cfw-rs/releases/download/v0.5.0/Clash.for.Mac_0.5.0_arm64.dmg",
    dmg_sha256: "fc9a1e62700a1b5f66ec42fcda0e5eb90efc052251321927f52b97c9223b7b93",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_is_internally_consistent() {
        assert!(MAC.dmg_url.ends_with(MAC.dmg_name));
        assert!(MAC.dmg_url.contains(&format!("/v{}/", MAC.version)));
        assert!(MAC.page_url.ends_with(&format!("/v{}", MAC.version)));
        assert_eq!(MAC.dmg_sha256.len(), 64);
        assert!(
            MAC.dmg_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
    }
}
