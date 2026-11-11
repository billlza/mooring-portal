//! The catalogue: what the portal sells, as the operator writes it.
//!
//! The operator makes each plan's price and payment page at Stripe, and
//! writes the same terms here, in a file the server reads when it starts.
//! The portal shows the terms, snapshots them into each order, and grants
//! a payment only if it is for exactly what the order's snapshot says; so
//! a catalogue that disagrees with Stripe grants nothing, and says why to
//! the operator.
//!
//! The file is the operator's own and holds no secret, so a refusal may
//! name the plan it is about.

use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;

use crate::json::Malformation;

/// The largest catalogue accepted, in bytes.
pub const MAX_CATALOGUE_BYTES: usize = 64 * 1024;
/// What a catalogue's `format` must say.
const FORMAT: &str = "mooring-portal-payments";
/// The one version of the format this build reads.
const VERSION: u32 = 1;
/// Plans in a catalogue at most.
const MAX_PLANS: usize = 24;
/// The longest a plan may run or a pack may stay valid: ten years.
const MAX_DAYS: u32 = 3_660;
/// The most traffic a plan may hold, in gigabytes.
const MAX_TRAFFIC_GB: u64 = 100_000;
/// The highest price, in the currency's hundredths.
const MAX_AMOUNT: u64 = 10_000_000;
/// Bytes in a gigabyte as quotas are set: decimal.
const GB: u64 = 1_000_000_000;
/// Where every payment page must be: Stripe's own host for Payment Links.
const PAYMENT_PAGES: &str = "https://buy.stripe.com/";
/// Where the page for managing a renewal must be: Stripe's customer
/// portal.
const MANAGE_PAGES: &str = "https://billing.stripe.com/p/login/";
/// Why a catalogue was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogueError {
    /// Larger than [`MAX_CATALOGUE_BYTES`].
    #[error("the catalogue is larger than {MAX_CATALOGUE_BYTES} bytes")]
    TooLarge,
    /// Not JSON, or not a catalogue's keys and types.
    #[error("the catalogue does not parse: {0}")]
    Malformed(Malformation),
    /// Not this format, or not a version this build reads.
    #[error("the catalogue is not {FORMAT:?} version {VERSION}")]
    Format,
    /// A currency the portal cannot show.
    #[error("the currency must be one of aud, cad, cny, eur, gbp, hkd, sgd, usd, in lowercase")]
    Currency,
    /// No plan, or too many.
    #[error("the catalogue must hold between 1 and {MAX_PLANS} plans")]
    PlanCount,
    /// A plan's id is not `a-z`, `0-9` and `-`.
    #[error("plan {0}: its id must be 1 to 32 of a-z, 0-9 and -, and neither begin nor end with -")]
    PlanId(usize),
    /// Two plans share an id.
    #[error("plan {0:?} is listed twice")]
    DuplicatePlan(String),
    /// Two plans share a payment page.
    #[error("plan {0:?} shares its payment link with another plan")]
    SharedLink(String),
    /// A plan's kind is not one the portal sells.
    #[error("plan {0:?}: its kind must be month, month_auto or traffic")]
    Kind(String),
    /// A plan's days are out of range.
    #[error("plan {0:?}: its days must be between 1 and {MAX_DAYS}")]
    Days(String),
    /// A plan's traffic is out of range.
    #[error("plan {0:?}: its traffic_gb must be between 1 and {MAX_TRAFFIC_GB}")]
    Traffic(String),
    /// A plan's price is out of range.
    #[error("plan {0:?}: its amount must be between 1 and {MAX_AMOUNT} hundredths")]
    Amount(String),
    /// A plan's payment page is not one of Stripe's Payment Links.
    #[error("plan {0:?}: its payment_link must be {PAYMENT_PAGES} and letters, digits or _")]
    PaymentLink(String),
    /// A plan's Payment Link id is not one.
    #[error("plan {0:?}: its payment_link_id must be plink_ and letters or digits")]
    PaymentLinkId(String),
    /// The page for managing a renewal is not Stripe's customer portal.
    #[error("manage_subscription_url must be {MANAGE_PAGES} and letters, digits or _")]
    ManageUrl,
    /// A plan renews by itself, but nothing says where to cancel it.
    #[error("a month_auto plan needs manage_subscription_url, where its user cancels the renewal")]
    NoManageUrl,
}

/// What a plan is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanKind {
    /// A month of service, paid once.
    Month,
    /// A month of service that Stripe charges again each month until its
    /// user cancels.
    MonthAuto,
    /// Extra traffic, valid for a number of days.
    Traffic,
}

impl PlanKind {
    /// Every kind, in the order the pages list them.
    pub const ALL: [Self; 3] = [Self::Month, Self::MonthAuto, Self::Traffic];

    /// The kind as the catalogue, the database and the feed write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Month => "month",
            Self::MonthAuto => "month_auto",
            Self::Traffic => "traffic",
        }
    }

    /// Reads what [`Self::as_str`] writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Whether Stripe charges for it again by itself.
    #[must_use]
    pub const fn renews(self) -> bool {
        matches!(self, Self::MonthAuto)
    }
}

/// A plan's id: 1 to 32 of `a-z`, `0-9` and `-`, neither beginning nor
/// ending with `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlanId(String);

impl PlanId {
    /// Validates `text` as a plan's id.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let valid = (1..=32).contains(&text.len())
            && text
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !text.starts_with('-')
            && !text.ends_with('-');
        valid.then(|| Self(text.to_owned()))
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A currency the portal prices in: one whose amounts are counted in
/// hundredths, which is how prices are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Currency {
    /// Australian dollars.
    Aud,
    /// Canadian dollars.
    Cad,
    /// Chinese yuan.
    Cny,
    /// Euros.
    Eur,
    /// Pounds sterling.
    Gbp,
    /// Hong Kong dollars.
    Hkd,
    /// Singapore dollars.
    Sgd,
    /// United States dollars.
    Usd,
}

impl Currency {
    /// Every currency, by code.
    pub const ALL: [Self; 8] = [
        Self::Aud,
        Self::Cad,
        Self::Cny,
        Self::Eur,
        Self::Gbp,
        Self::Hkd,
        Self::Sgd,
        Self::Usd,
    ];

    /// Reads a currency's code: three lowercase letters, as Stripe writes
    /// them.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|currency| currency.as_str() == text)
    }

    /// The code, in lowercase.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Aud => "aud",
            Self::Cad => "cad",
            Self::Cny => "cny",
            Self::Eur => "eur",
            Self::Gbp => "gbp",
            Self::Hkd => "hkd",
            Self::Sgd => "sgd",
            Self::Usd => "usd",
        }
    }
}

/// One thing the portal sells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Its id, which orders and the feed name it by.
    pub id: PlanId,
    /// What it is.
    pub kind: PlanKind,
    /// The days of service it buys, or the days its traffic stays valid.
    pub days: u32,
    /// Its traffic: a month's quota for each billing cycle, or a pack's
    /// extra traffic.
    pub traffic_bytes: u64,
    /// Its price, in the currency's hundredths, as Stripe counts amounts.
    pub amount: u64,
    /// Its currency.
    pub currency: Currency,
    /// Its payment page at Stripe.
    pub payment_link: String,
    /// The id Stripe gives for that page with each payment made on it.
    pub payment_link_id: String,
}

/// The operator's catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    livemode: bool,
    plans: Vec<Plan>,
    manage_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    format: String,
    version: u32,
    livemode: bool,
    currency: String,
    plans: Vec<WrittenPlan>,
    manage_subscription_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenPlan {
    id: String,
    kind: String,
    days: u32,
    traffic_gb: u64,
    amount: u64,
    payment_link: String,
    payment_link_id: String,
}

impl Catalogue {
    /// Reads the operator's file.
    pub fn parse(document: &str) -> Result<Self, CatalogueError> {
        if document.len() > MAX_CATALOGUE_BYTES {
            return Err(CatalogueError::TooLarge);
        }
        let written: Written = serde_json::from_str(document)
            .map_err(|error| CatalogueError::Malformed(Malformation::of(&error)))?;
        if written.format != FORMAT || written.version != VERSION {
            return Err(CatalogueError::Format);
        }
        let currency = Currency::parse(&written.currency).ok_or(CatalogueError::Currency)?;
        if written.plans.is_empty() || written.plans.len() > MAX_PLANS {
            return Err(CatalogueError::PlanCount);
        }
        let mut plans = Vec::with_capacity(written.plans.len());
        let (mut ids, mut links) = (HashSet::new(), HashSet::new());
        for (index, plan) in written.plans.into_iter().enumerate() {
            let id = PlanId::parse(&plan.id).ok_or(CatalogueError::PlanId(index + 1))?;
            let name = || plan.id.clone();
            let kind = PlanKind::parse(&plan.kind).ok_or_else(|| CatalogueError::Kind(name()))?;
            if !(1..=MAX_DAYS).contains(&plan.days) {
                return Err(CatalogueError::Days(name()));
            }
            if !(1..=MAX_TRAFFIC_GB).contains(&plan.traffic_gb) {
                return Err(CatalogueError::Traffic(name()));
            }
            if !(1..=MAX_AMOUNT).contains(&plan.amount) {
                return Err(CatalogueError::Amount(name()));
            }
            if !page(&plan.payment_link, PAYMENT_PAGES) {
                return Err(CatalogueError::PaymentLink(name()));
            }
            let link_id = plan.payment_link_id.strip_prefix("plink_");
            if !link_id.is_some_and(|rest| word(rest, false)) {
                return Err(CatalogueError::PaymentLinkId(name()));
            }
            if !ids.insert(plan.id.clone()) {
                return Err(CatalogueError::DuplicatePlan(name()));
            }
            // Two plans on one page, or on one page's id, could not be told
            // apart when a payment comes back.
            let fresh = links.insert(plan.payment_link.clone())
                & links.insert(plan.payment_link_id.clone());
            if !fresh {
                return Err(CatalogueError::SharedLink(name()));
            }
            plans.push(Plan {
                id,
                kind,
                days: plan.days,
                traffic_bytes: plan.traffic_gb * GB,
                amount: plan.amount,
                currency,
                payment_link: plan.payment_link,
                payment_link_id: plan.payment_link_id,
            });
        }
        let manage_url = written.manage_subscription_url;
        if manage_url
            .as_deref()
            .is_some_and(|url| !page(url, MANAGE_PAGES))
        {
            return Err(CatalogueError::ManageUrl);
        }
        if manage_url.is_none() && plans.iter().any(|plan| plan.kind.renews()) {
            return Err(CatalogueError::NoManageUrl);
        }
        Ok(Self {
            livemode: written.livemode,
            plans,
            manage_url,
        })
    }

    /// Whether the catalogue's payment pages take real payments. Stripe
    /// marks each event the same way, and the portal acts only on events
    /// of the catalogue's kind.
    #[must_use]
    pub const fn livemode(&self) -> bool {
        self.livemode
    }

    /// The plans, in the operator's order.
    #[must_use]
    pub fn plans(&self) -> &[Plan] {
        &self.plans
    }

    /// The plan with `id`.
    #[must_use]
    pub fn plan(&self, id: &str) -> Option<&Plan> {
        self.plans.iter().find(|plan| plan.id.as_str() == id)
    }

    /// Where a user manages a renewal: Stripe's customer portal.
    #[must_use]
    pub fn manage_url(&self) -> Option<&str> {
        self.manage_url.as_deref()
    }
}

/// Whether `url` is `prefix` and then a word: a page of that host and
/// path, with no query or fragment of its own.
fn page(url: &str, prefix: &str) -> bool {
    url.strip_prefix(prefix)
        .is_some_and(|rest| word(rest, true))
}

/// Whether `text` is 1 to 128 ASCII letters and digits, and `_` if
/// `underscores` are allowed, as Stripe writes its ids and its pages.
fn word(text: &str, underscores: bool) -> bool {
    (1..=128).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || (underscores && byte == b'_'))
}

#[cfg(test)]
mod tests;
