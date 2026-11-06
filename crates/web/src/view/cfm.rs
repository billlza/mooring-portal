//! What the subscription page's CFM panel says.

use portal_store::CfmDelivery;

use crate::format::beijing;
use crate::i18n::{Strings, fill};

/// The CFM panel of a bound account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CfmPanel {
    /// The account has no profile now.
    Waiting {
        /// "Your link is paused …", if the account still has a link: Mooring
        /// withholds the user for a passing reason, and the link works
        /// again once a profile is delivered.
        paused: Option<String>,
    },
    /// There is a profile to fetch.
    Ready {
        /// "Profile generated …".
        generated: String,
        /// "Your current link was made …", or `None` without a link.
        link: Option<String>,
    },
}

impl CfmPanel {
    /// The panel for `delivery`, in the language of `t`.
    pub(crate) fn of(delivery: CfmDelivery, t: &Strings) -> Self {
        match delivery {
            CfmDelivery::NoProfile {
                paused_link_created_at,
            } => Self::Waiting {
                paused: paused_link_created_at.map(|at| fill(t.cfm.link_paused, beijing(at))),
            },
            CfmDelivery::Profile {
                generated_at,
                link_created_at,
            } => Self::Ready {
                generated: fill(t.cfm.generated, beijing(generated_at)),
                link: link_created_at.map(|at| fill(t.cfm.link_created, beijing(at))),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use portal_core::time::UnixTime;

    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn the_panel_names_the_profile_and_the_link_in_beijing_time() {
        let t = Lang::Zh.strings();
        let generated_at = UnixTime::from_secs(1_793_955_600).unwrap();
        let link = UnixTime::from_secs(1_793_959_200).unwrap();
        assert_eq!(
            CfmPanel::of(
                CfmDelivery::Profile {
                    generated_at,
                    link_created_at: Some(link)
                },
                t
            ),
            CfmPanel::Ready {
                generated: "配置生成于 2026-11-06 17:00".to_owned(),
                link: Some(
                    "当前链接生成于 2026-11-06 18:00。链接只在生成时显示一次；忘了就重新生成，旧链接会立即失效。"
                        .to_owned()
                ),
            }
        );
        assert_eq!(
            CfmPanel::of(
                CfmDelivery::Profile {
                    generated_at,
                    link_created_at: None
                },
                t
            ),
            CfmPanel::Ready {
                generated: "配置生成于 2026-11-06 17:00".to_owned(),
                link: None,
            }
        );
        assert_eq!(
            CfmPanel::of(
                CfmDelivery::NoProfile {
                    paused_link_created_at: None
                },
                t
            ),
            CfmPanel::Waiting { paused: None }
        );
        assert_eq!(
            CfmPanel::of(
                CfmDelivery::NoProfile {
                    paused_link_created_at: Some(link)
                },
                t
            ),
            CfmPanel::Waiting {
                paused: Some(
                    "你的订阅链接生成于 2026-11-06 18:00，现在暂停，取不到配置。运营者恢复为你提供配置后，在 CFM 里对这份配置点「更新」即可，不用重新生成。"
                        .to_owned()
                )
            }
        );
    }
}
