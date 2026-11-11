//! The brand: the only place the portal's name and slogan are written.
//!
//! Renaming the portal means editing this file and nothing else: pages,
//! titles and the logo take the name from here, the logo mark is a
//! lighthouse with no letters in it, and no crate, binary or path carries
//! the name.

use crate::i18n::Lang;

/// The brand in one language.
#[derive(Debug)]
pub(crate) struct Brand {
    /// The name.
    pub(crate) name: &'static str,
    /// The name in Latin letters, shown under the Chinese name.
    pub(crate) latin: &'static str,
    /// The headline on the home page, a line to each part.
    pub(crate) slogan: [&'static str; 2],
    /// One line on what the name means, for a page's description.
    pub(crate) tagline: &'static str,
}

const ZH: Brand = Brand {
    name: "泊光",
    latin: "Moorlight",
    slogan: ["让连接，", "有处停泊。"],
    tagline: "夜里引船进港停泊的那盏灯。",
};

const EN: Brand = Brand {
    name: "Moorlight",
    latin: "Moorlight",
    slogan: ["A place for your", "connection to moor."],
    tagline: "The light that brings a boat in to its mooring at night.",
};

/// The brand in `lang`.
pub(crate) const fn brand(lang: Lang) -> &'static Brand {
    match lang {
        Lang::Zh => &ZH,
        Lang::En => &EN,
    }
}
