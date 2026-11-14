//! What the download page says about the CFM release on offer.
//!
//! The release is the one the host last imported from GitHub. The page
//! says when that was last found to be the latest, and says so plainly
//! when there is no check on record within the last day: a host that has
//! stopped checking must not go on looking current. That covers a check
//! dated ahead of the clock, which was made while the clock was wrong and
//! so says nothing of its age. With no release imported yet the page says
//! that, and never falls back to one written into the program.

use portal_core::release::{REPOSITORY, Version};
use portal_core::time::UnixTime;
use portal_store::OfferedRelease;

use crate::format::{beijing, beijing_date};
use crate::i18n::{Strings, fill};

/// A check older than this is said to be out of date.
pub(crate) const STALE_AFTER_SECS: u64 = 24 * 3600;
/// A check this far ahead of the clock is still taken at its word: a clock
/// is set right in small steps.
pub(crate) const AHEAD_SECS: u64 = 5 * 60;

/// The download page's macOS panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReleaseView {
    /// No release has been imported yet.
    NotYet {
        /// Where the latest release can be found meanwhile.
        releases_url: String,
    },
    /// The release on offer.
    Offered(Box<Offered>),
}

/// A release, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Offered {
    pub(crate) version: String,
    /// The day it was published, in Beijing.
    pub(crate) published: String,
    pub(crate) page_url: String,
    pub(crate) dmg_name: String,
    pub(crate) dmg_url: String,
    pub(crate) dmg_sha256: String,
    /// "Release details checked at …".
    pub(crate) checked: String,
    /// The warning that no check within the last day is on record, when
    /// none is.
    pub(crate) stale: Option<&'static str>,
}

impl ReleaseView {
    /// The view of `offered` at `now`, in the language of `t`.
    pub(crate) fn of(offered: Option<OfferedRelease>, now: UnixTime, t: &Strings) -> Self {
        let Some(OfferedRelease {
            release,
            checked_at,
        }) = offered
        else {
            return Self::NotYet {
                releases_url: format!("{REPOSITORY}/releases/latest"),
            };
        };
        Self::Offered(Box::new(Offered {
            version: shown_version(release.version(), t),
            published: beijing_date(release.published_at()),
            page_url: release.page_url(),
            dmg_name: release.dmg_name(),
            dmg_url: release.dmg_url(),
            dmg_sha256: release.dmg_sha256().to_owned(),
            checked: fill(t.download.checked, beijing(checked_at)),
            stale: (now.secs_since(checked_at) > STALE_AFTER_SECS
                || checked_at.secs_since(now) > AHEAD_SECS)
                .then_some(t.download.stale),
        }))
    }
}

/// A version as the page writes it: a rebuild's build number in words
/// after the version it rebuilds, as CFM names such a release.
fn shown_version(version: Version, t: &Strings) -> String {
    match version.build() {
        Some(build) => fill(&fill(t.download.rebuild, version.without_build()), build),
        None => version.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use portal_core::release::CfmRelease;

    use super::*;
    use crate::i18n::Lang;

    const NOW: u64 = 1_793_880_000;
    const SHA: &str = "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f";

    fn at(secs: u64) -> UnixTime {
        UnixTime::from_secs(secs).unwrap()
    }

    fn offered(checked_at: u64) -> OfferedRelease {
        OfferedRelease {
            release: CfmRelease::from_parts(
                Version::parse("0.5.1").unwrap(),
                UnixTime::parse_utc("2026-11-04T18:00:00Z").unwrap(),
                SHA,
            )
            .unwrap(),
            checked_at: at(checked_at),
        }
    }

    #[test]
    fn with_nothing_imported_the_page_points_at_the_releases() {
        let t = Lang::Zh.strings();
        assert_eq!(
            ReleaseView::of(None, at(NOW), t),
            ReleaseView::NotYet {
                releases_url: "https://github.com/billlza/cfw-rs/releases/latest".to_owned()
            }
        );
    }

    #[test]
    fn a_release_is_shown_with_its_addresses_and_when_it_was_checked() {
        let t = Lang::Zh.strings();
        let ReleaseView::Offered(shown) = ReleaseView::of(Some(offered(NOW)), at(NOW), t) else {
            panic!("a release was offered");
        };
        assert_eq!(
            *shown,
            Offered {
                version: "0.5.1".to_owned(),
                // 18:00 UTC on the 4th is the 5th in Beijing.
                published: "2026-11-05".to_owned(),
                page_url: "https://github.com/billlza/cfw-rs/releases/tag/v0.5.1".to_owned(),
                dmg_name: "Clash.for.Mac_0.5.1_arm64.dmg".to_owned(),
                dmg_url: "https://github.com/billlza/cfw-rs/releases/download/v0.5.1/\
                          Clash.for.Mac_0.5.1_arm64.dmg"
                    .to_owned(),
                dmg_sha256: SHA.to_owned(),
                checked: "发布信息核对于 2026-11-05 20:00".to_owned(),
                stale: None,
            }
        );
    }

    #[test]
    fn a_rebuild_is_shown_with_its_build_number_and_its_own_addresses() {
        let rebuild = OfferedRelease {
            release: CfmRelease::from_parts(
                Version::parse("0.5.1-build.51006").unwrap(),
                UnixTime::parse_utc("2026-11-12T19:03:23Z").unwrap(),
                SHA,
            )
            .unwrap(),
            checked_at: at(NOW),
        };
        for (lang, version) in [
            (Lang::Zh, "0.5.1（build 51006）"),
            (Lang::En, "0.5.1 (build 51006)"),
        ] {
            let ReleaseView::Offered(shown) =
                ReleaseView::of(Some(rebuild.clone()), at(NOW), lang.strings())
            else {
                panic!("a release was offered");
            };
            assert_eq!(shown.version, version);
            assert_eq!(
                shown.page_url,
                "https://github.com/billlza/cfw-rs/releases/tag/v0.5.1-build.51006"
            );
            assert_eq!(shown.dmg_name, "Clash.for.Mac_0.5.1_arm64.dmg");
            assert_eq!(
                shown.dmg_url,
                "https://github.com/billlza/cfw-rs/releases/download/v0.5.1-build.51006/\
                 Clash.for.Mac_0.5.1_arm64.dmg"
            );
        }
    }

    #[test]
    fn a_check_more_than_a_day_old_is_said_to_be_out_of_date() {
        for lang in [Lang::Zh, Lang::En] {
            let t = lang.strings();
            let stale = |age: u64| match ReleaseView::of(Some(offered(NOW)), at(NOW + age), t) {
                ReleaseView::Offered(shown) => shown.stale,
                ReleaseView::NotYet { .. } => panic!("a release was offered"),
            };
            assert_eq!(stale(0), None);
            assert_eq!(stale(STALE_AFTER_SECS), None);
            assert_eq!(stale(STALE_AFTER_SECS + 1), Some(t.download.stale));
            // A check a little ahead of the clock is a clock being set
            // right. One far ahead was made when the clock was wrong, and
            // says nothing of how old it is: it must not look current.
            let ahead = |by: u64| match ReleaseView::of(Some(offered(NOW + by)), at(NOW), t) {
                ReleaseView::Offered(shown) => shown.stale,
                ReleaseView::NotYet { .. } => panic!("a release was offered"),
            };
            assert_eq!(ahead(60), None);
            assert_eq!(ahead(AHEAD_SECS), None);
            assert_eq!(ahead(AHEAD_SECS + 1), Some(t.download.stale));
            assert_eq!(ahead(400 * 86_400), Some(t.download.stale));
        }
        assert_eq!(STALE_AFTER_SECS, 86_400);
        assert_eq!(AHEAD_SECS, 300);
    }
}
