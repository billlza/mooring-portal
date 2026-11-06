//! The brand: the only place the portal's name and slogan are written.
//!
//! Renaming the portal means editing this file and nothing else: pages,
//! titles and the logo take the name from here, the logo mark is a
//! lantern with no letters in it, and no crate, binary or path carries the
//! name.

use crate::i18n::Lang;

/// The brand in one language.
#[derive(Debug)]
pub(crate) struct Brand {
    /// The name.
    pub(crate) name: &'static str,
    /// The name in Latin letters, shown under the Chinese name.
    pub(crate) latin: &'static str,
    /// The headline on the home page.
    pub(crate) slogan: &'static str,
    /// One line on what the name means, for the footer and descriptions.
    pub(crate) tagline: &'static str,
}

const ZH: Brand = Brand {
    name: "泊光",
    latin: "Moorlight",
    slogan: "小而稳、看得清、不窥探",
    tagline: "夜里停泊的船上，那盏一直亮着的灯。",
};

const EN: Brand = Brand {
    name: "Moorlight",
    latin: "Moorlight",
    slogan: "Small, steady and discreet",
    tagline: "The light that stays on aboard a boat moored for the night.",
};

/// The brand in `lang`.
pub(crate) const fn brand(lang: Lang) -> &'static Brand {
    match lang {
        Lang::Zh => &ZH,
        Lang::En => &EN,
    }
}
