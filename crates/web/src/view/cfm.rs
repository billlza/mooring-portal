//! What the subscription page's CFM panel says.

use portal_store::CfmDelivery;

use crate::format::beijing;
use crate::i18n::{Strings, fill};

/// The CFM panel of a bound account. An account has a link only while it
/// has a profile, so a panel without a profile has no link to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CfmPanel {
    /// The operator has uploaded no profile for the account.
    Waiting,
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
            CfmDelivery::NoProfile => Self::Waiting,
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
        assert_eq!(CfmPanel::of(CfmDelivery::NoProfile, t), CfmPanel::Waiting);
    }
}
