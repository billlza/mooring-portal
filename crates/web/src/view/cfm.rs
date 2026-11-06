//! What the subscription page's CFM panel says.

use portal_core::time::UnixTime;

use crate::format::beijing;
use crate::i18n::{Strings, fill};

/// The CFM panel of a bound account: whether there is a profile to fetch,
/// and whether the account has a subscription link. A link can outlive its
/// profile (a later bundle may leave the user out), so either may be
/// present without the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CfmPanel {
    /// "Profile generated …", or `None` while the operator has uploaded none.
    pub(crate) generated: Option<String>,
    /// "Your current link was made …", or `None` without a link.
    pub(crate) link: Option<String>,
}

impl CfmPanel {
    /// The panel for a profile generated at `profile` and a link made at
    /// `link`, in the language of `t`.
    pub(crate) fn of(profile: Option<UnixTime>, link: Option<UnixTime>, t: &Strings) -> Self {
        Self {
            generated: profile.map(|at| fill(t.cfm.generated, beijing(at))),
            link: link.map(|at| fill(t.cfm.link_created, beijing(at))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn the_panel_names_the_profile_and_the_link_in_beijing_time() {
        let t = Lang::Zh.strings();
        let profile = UnixTime::from_secs(1_793_955_600).unwrap();
        let link = UnixTime::from_secs(1_793_959_200).unwrap();
        assert_eq!(
            CfmPanel::of(Some(profile), Some(link), t),
            CfmPanel {
                generated: Some("配置生成于 2026-11-06 17:00".to_owned()),
                link: Some(
                    "当前链接生成于 2026-11-06 18:00。链接只在生成时显示一次；忘了就重新生成，旧链接会立即失效。"
                        .to_owned()
                ),
            }
        );
        assert_eq!(
            CfmPanel::of(None, None, t),
            CfmPanel {
                generated: None,
                link: None
            }
        );
    }
}
