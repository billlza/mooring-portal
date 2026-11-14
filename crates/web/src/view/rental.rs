//! What the Mac rental page says: the machines on offer with their price
//! for every period, the periods, and the signed-in account's bookings.
//!
//! A booking is shown on the terms it was made on, which may no longer be
//! the offer's.

use portal_core::payment::Currency;
use portal_core::rental::{BookingTerms, FIRST_MONTH_PERCENT, Machine, Period};
use portal_store::Booking;

use crate::format::{beijing, money};
use crate::i18n::{Strings, fill};

/// The period the page starts on.
const FIRST_CHOSEN: Period = Period::Month;

/// A machine on offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MachineView {
    /// Its id, as the booking form names it.
    pub(crate) id: &'static str,
    pub(crate) chip: &'static str,
    /// Its cores, memory, storage and network port, a line each.
    pub(crate) specs: [String; 4],
    /// Its price for every period, in the order of [`Period::ALL`].
    pub(crate) prices: Vec<PriceView>,
}

/// A machine's price for a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PriceView {
    /// The period's id, which the page shows the price by.
    pub(crate) period: &'static str,
    pub(crate) amount: String,
    /// What the amount is for: `/月`.
    pub(crate) per: &'static str,
    /// The first period's price, where it is less.
    pub(crate) first: Option<String>,
}

/// A period to choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeriodView {
    /// Its id, as the booking form names it.
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    /// What choosing it saves, if anything.
    pub(crate) saving: Option<String>,
    /// Whether the page starts with it chosen.
    pub(crate) chosen: bool,
}

/// One of the account's bookings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BookingView {
    /// Its machine's id, as the withdrawal form names it.
    pub(crate) machine: String,
    /// The machine: chip, memory and storage.
    pub(crate) name: String,
    pub(crate) period: &'static str,
    /// The price for each period.
    pub(crate) price: String,
    /// The first period's price, where it is less.
    pub(crate) first: Option<String>,
    /// When it was made, in Beijing time.
    pub(crate) placed: String,
}

impl MachineView {
    pub(crate) fn of(machine: &'static Machine, t: &Strings) -> Self {
        let r = &t.rental;
        Self {
            id: machine.id,
            chip: machine.chip,
            specs: [
                format!(
                    "{} · {}",
                    fill(r.cpu, machine.cpu_cores),
                    fill(r.gpu, machine.gpu_cores)
                ),
                fill(r.memory, machine.memory_gb),
                fill(r.storage, machine.storage_gb),
                fill(r.ethernet, gigabits(machine.ethernet_mbps)),
            ],
            prices: Period::ALL
                .into_iter()
                .map(|period| PriceView {
                    period: period.as_str(),
                    amount: pounds(machine.price(period)),
                    per: per(period, t),
                    first: first(machine.price(period), machine.first_price(period), t),
                })
                .collect(),
        }
    }
}

impl PeriodView {
    /// Every period, in the order of [`Period::ALL`].
    pub(crate) fn all(t: &Strings) -> Vec<Self> {
        Period::ALL
            .into_iter()
            .map(|period| Self {
                id: period.as_str(),
                label: label(period, t),
                saving: match (period, period.saving_percent()) {
                    (Period::Month, _) => {
                        Some(fill(t.rental.first_month_saving, 100 - FIRST_MONTH_PERCENT))
                    }
                    (_, 0) => None,
                    (_, percent) => Some(fill(t.rental.saving, percent)),
                },
                chosen: period == FIRST_CHOSEN,
            })
            .collect()
    }
}

impl BookingView {
    pub(crate) fn of(booking: &Booking, t: &Strings) -> Self {
        let BookingTerms {
            machine,
            chip,
            memory_gb,
            storage_gb,
            period,
            price,
            first_price,
        } = &booking.terms;
        Self {
            machine: machine.to_string(),
            name: format!("{chip} · {memory_gb}GB · {storage_gb}GB"),
            period: label(*period, t),
            price: format!("{}{}", pounds(*price), per(*period, t)),
            first: first(*price, *first_price, t),
            placed: fill(t.rental.placed, beijing(booking.placed_at)),
        }
    }
}

/// `pence` as a price in pounds: `£79.00`.
fn pounds(pence: u64) -> String {
    money(pence, Currency::Gbp)
}

/// What the first period costs, where it is less than `price`.
fn first(price: u64, first_price: u64, t: &Strings) -> Option<String> {
    (first_price < price).then(|| fill(t.rental.first_month, pounds(first_price)))
}

/// `period`'s name.
const fn label(period: Period, t: &Strings) -> &'static str {
    match period {
        Period::Day => t.rental.day,
        Period::Week => t.rental.week,
        Period::Month => t.rental.month,
        Period::Quarter => t.rental.quarter,
        Period::Year => t.rental.year,
    }
}

/// What a price for `period` is for: `/月`.
const fn per(period: Period, t: &Strings) -> &'static str {
    match period {
        Period::Day => t.rental.per_day,
        Period::Week => t.rental.per_week,
        Period::Month => t.rental.per_month,
        Period::Quarter => t.rental.per_quarter,
        Period::Year => t.rental.per_year,
    }
}

/// A port's speed in gigabits, as Apple writes it: `2.5Gb`, `10Gb`.
fn gigabits(mbps: u16) -> String {
    let whole = mbps / 1_000;
    match mbps % 1_000 {
        0 => format!("{whole}Gb"),
        rest => {
            let fraction = format!("{rest:03}");
            format!("{whole}.{}Gb", fraction.trim_end_matches('0'))
        }
    }
}

#[cfg(test)]
mod tests {
    use portal_core::rental::{ChipName, MachineId};
    use portal_core::time::UnixTime;

    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn a_machine_shows_its_specifications_and_every_price() {
        let t = Lang::Zh.strings();
        let view = MachineView::of(Machine::find("m6-24-512").unwrap(), t);
        assert_eq!(view.id, "m6-24-512");
        assert_eq!(view.chip, "M6");
        assert_eq!(
            view.specs,
            [
                "12 核 CPU · 12 核 GPU",
                "24GB 统一内存",
                "512GB 固态硬盘",
                "2.5Gb 以太网"
            ]
        );
        let prices: Vec<(&str, &str, &str, Option<&str>)> = view
            .prices
            .iter()
            .map(|price| {
                (
                    price.period,
                    price.amount.as_str(),
                    price.per,
                    price.first.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            prices,
            [
                ("day", "£22.00", "/天", None),
                ("week", "£74.00", "/周", None),
                ("month", "£169.00", "/月", Some("首月 £101.40")),
                ("quarter", "£456.00", "/季", None),
                ("year", "£1724.00", "/年", None),
            ]
        );
        let en = MachineView::of(Machine::find("m4-16-256").unwrap(), Lang::En.strings());
        assert_eq!(
            en.specs,
            [
                "10-core CPU · 10-core GPU",
                "16GB unified memory",
                "256GB SSD",
                "1Gb Ethernet"
            ]
        );
        assert_eq!(en.prices[2].first.as_deref(), Some("first month £47.40"));
        assert_eq!(en.prices[0].per, "/day");
    }

    #[test]
    fn the_periods_say_what_they_save_and_the_month_is_chosen() {
        let periods = PeriodView::all(Lang::Zh.strings());
        let shown: Vec<(&str, &str, Option<&str>, bool)> = periods
            .iter()
            .map(|period| {
                (
                    period.id,
                    period.label,
                    period.saving.as_deref(),
                    period.chosen,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("day", "按天", None, false),
                ("week", "按周", None, false),
                ("month", "按月", Some("首月省 40%"), true),
                ("quarter", "按季", Some("省 10%"), false),
                ("year", "按年", Some("省 15%"), false),
            ]
        );
        let en = PeriodView::all(Lang::En.strings());
        assert_eq!(en[2].saving.as_deref(), Some("40% off month one"));
        assert_eq!(en[4].saving.as_deref(), Some("save 15%"));
        assert_eq!(en[4].label, "Yearly");
    }

    #[test]
    fn a_booking_shows_the_terms_it_was_made_on() {
        // A machine no longer on offer, at a price the offer never had.
        let booking = Booking {
            id: 4,
            terms: BookingTerms {
                machine: MachineId::parse("m3-8-256").unwrap(),
                chip: ChipName::parse("M3").unwrap(),
                memory_gb: 8,
                storage_gb: 256,
                period: Period::Month,
                price: 5_000,
                first_price: 3_000,
            },
            placed_at: UnixTime::from_secs(1_793_880_000).unwrap(),
        };
        let view = BookingView::of(&booking, Lang::Zh.strings());
        assert_eq!(
            view,
            BookingView {
                machine: "m3-8-256".to_owned(),
                name: "M3 · 8GB · 256GB".to_owned(),
                period: "按月",
                price: "£50.00/月".to_owned(),
                first: Some("首月 £30.00".to_owned()),
                placed: "预订于 2026-11-05 20:00".to_owned(),
            }
        );
        let yearly = Booking {
            terms: BookingTerms {
                period: Period::Year,
                price: 60_000,
                first_price: 60_000,
                ..booking.terms.clone()
            },
            ..booking
        };
        let view = BookingView::of(&yearly, Lang::En.strings());
        assert_eq!(
            (view.period, view.price.as_str(), view.first),
            ("Yearly", "£600.00/year", None)
        );
        assert_eq!(view.placed, "Booked 2026-11-05 20:00");
    }

    #[test]
    fn port_speeds_are_written_as_apple_writes_them() {
        for (mbps, text) in [
            (1_000, "1Gb"),
            (2_500, "2.5Gb"),
            (10_000, "10Gb"),
            (2_250, "2.25Gb"),
            (100, "0.1Gb"),
        ] {
            assert_eq!(gigabits(mbps), text, "{mbps}");
        }
    }
}
