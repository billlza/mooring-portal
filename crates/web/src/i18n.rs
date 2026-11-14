//! Languages and the text of every page.
//!
//! Each language is one value of [`Strings`], so a phrase missing from
//! either language is a compile error. Chinese is the default. Phrases with
//! a value take it through [`fill`] at a `{}` mark.

mod en;
mod text;
mod zh;

use serde::Deserialize;

pub(crate) use text::{Item, Strings};

/// A page language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Lang {
    /// Simplified Chinese, the default.
    Zh,
    /// English.
    En,
}

impl Lang {
    /// The language a `__Host-lang` cookie names; Chinese when there is no
    /// cookie or it names nothing known.
    pub(crate) fn from_cookie(value: Option<&str>) -> Self {
        match value {
            Some("en") => Self::En,
            _ => Self::Zh,
        }
    }

    /// The cookie and form value.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    /// The HTML `lang` attribute.
    pub(crate) const fn html_lang(self) -> &'static str {
        match self {
            Self::Zh => "zh-CN",
            Self::En => "en",
        }
    }

    /// The language the switch offers.
    pub(crate) const fn other(self) -> Self {
        match self {
            Self::Zh => Self::En,
            Self::En => Self::Zh,
        }
    }

    /// The text of every page.
    pub(crate) const fn strings(self) -> &'static Strings {
        match self {
            Self::Zh => &zh::STRINGS,
            Self::En => &en::STRINGS,
        }
    }
}

/// `phrase` with its `{}` mark replaced by `value`.
pub(crate) fn fill(phrase: &str, value: impl std::fmt::Display) -> String {
    phrase.replacen("{}", &value.to_string(), 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_is_the_default() {
        assert_eq!(Lang::from_cookie(None), Lang::Zh);
        assert_eq!(Lang::from_cookie(Some("fr")), Lang::Zh);
        assert_eq!(Lang::from_cookie(Some("en")), Lang::En);
        assert_eq!(Lang::Zh.other(), Lang::En);
        assert_eq!(Lang::En.other(), Lang::Zh);
        assert_eq!(Lang::En.code(), "en");
        assert_eq!(Lang::Zh.html_lang(), "zh-CN");
    }

    #[test]
    fn phrases_take_their_value_once() {
        assert_eq!(fill("还剩 {} 天", 3), "还剩 3 天");
        assert_eq!(fill("{} and {}", 1), "1 and {}");
    }

    #[test]
    fn every_phrase_with_a_value_has_a_mark() {
        for lang in [Lang::Zh, Lang::En] {
            let t = lang.strings();
            for phrase in [
                t.auth.too_many_attempts,
                t.account.bound_since,
                t.subscription.awaiting_body,
                t.subscription.since,
                t.subscription.counted_through,
                t.subscription.quota_used,
                t.subscription.days_left_one,
                t.subscription.days_left_many,
                t.subscription.as_of,
                t.subscription.resets,
                t.download.checked,
                t.pay.note_refunded_part,
                t.rental.cpu,
                t.rental.gpu,
                t.rental.memory,
                t.rental.storage,
                t.rental.ethernet,
                t.rental.saving,
                t.rental.first_month_saving,
                t.rental.first_month,
                t.rental.placed,
            ] {
                assert_eq!(phrase.matches("{}").count(), 1, "{lang:?}: {phrase}");
            }
            // A plan's terms take its days, then its traffic; a pack's take
            // its traffic alone, since its days are not applied.
            for phrase in [t.pay.terms_month, t.pay.terms_month_auto] {
                assert_eq!(phrase.matches("{}").count(), 2, "{lang:?}: {phrase}");
                assert_eq!(fill(&fill(phrase, 30), "5 GB").matches("{}").count(), 0);
            }
            assert_eq!(t.pay.terms_traffic.matches("{}").count(), 1, "{lang:?}");
        }
    }
}
