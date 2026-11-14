//! Renting a Mac mini: the machines on offer, the periods they are billed
//! by, and what a booking asks for.
//!
//! The operator lets whole machines, each to one customer, for a day at
//! the least: Apple's licence for macOS lets a Mac be leased to one person
//! for 24 hours or more. A machine is bought from Apple once it is booked,
//! so a rental starts the next day at the earliest (the operator's
//! decision of 2026-11-14).
//!
//! The prices are the operator's (2026-11-14): a month of the M4 at £79;
//! the newer machines priced to pay for themselves in twelve months; the
//! first month of a monthly rental at 60 per cent of its price; a quarter
//! and a year paid ahead at 10 and 15 per cent off their months. They are
//! written here rather than in the payments catalogue because a booking
//! pays nothing: it asks the operator for a machine, and the operator
//! settles the payment with whoever booked it.
//!
//! A booking keeps what it asked for as the offer stood ([`BookingTerms`]),
//! so a machine or price that later leaves this list leaves the bookings
//! made on it readable.

use std::fmt;

/// What the first month of a monthly rental costs, as a percentage of a
/// month's price.
pub const FIRST_MONTH_PERCENT: u64 = 60;

/// A Mac mini on offer, as Apple sells it, and its prices.
#[derive(Debug, PartialEq, Eq)]
pub struct Machine {
    /// How forms and the database name it.
    pub id: &'static str,
    /// Apple's name for its chip.
    pub chip: &'static str,
    /// The chip's CPU cores.
    pub cpu_cores: u8,
    /// The chip's GPU cores.
    pub gpu_cores: u8,
    /// Its unified memory, in gigabytes.
    pub memory_gb: u16,
    /// Its storage, in gigabytes.
    pub storage_gb: u16,
    /// Its Ethernet port's speed, in megabits a second.
    pub ethernet_mbps: u16,
    /// A day's price, in pence.
    day: u64,
    /// A week's price, in pence.
    week: u64,
    /// A month's price, in pence; a quarter's and a year's follow from it.
    month: u64,
}

/// The machines on offer, in the order the page lists them. Apple's
/// specifications as its UK store gave them on 2026-11-14: the M6 and the
/// M5 Pro are the models sold since September 2026; the M4 is the one
/// before them.
pub const MACHINES: [Machine; 4] = [
    Machine {
        id: "m4-16-256",
        chip: "M4",
        cpu_cores: 10,
        gpu_cores: 10,
        memory_gb: 16,
        storage_gb: 256,
        ethernet_mbps: 1_000,
        day: 1_000,
        week: 3_500,
        month: 7_900,
    },
    Machine {
        id: "m6-16-256",
        chip: "M6",
        cpu_cores: 12,
        gpu_cores: 12,
        memory_gb: 16,
        storage_gb: 256,
        ethernet_mbps: 2_500,
        day: 1_500,
        week: 5_200,
        month: 11_900,
    },
    Machine {
        id: "m6-24-512",
        chip: "M6",
        cpu_cores: 12,
        gpu_cores: 12,
        memory_gb: 24,
        storage_gb: 512,
        ethernet_mbps: 2_500,
        day: 2_200,
        week: 7_400,
        month: 16_900,
    },
    Machine {
        id: "m5pro-24-512",
        chip: "M5 Pro",
        cpu_cores: 15,
        gpu_cores: 16,
        memory_gb: 24,
        storage_gb: 512,
        ethernet_mbps: 2_500,
        day: 2_800,
        week: 9_600,
        month: 21_900,
    },
];

impl Machine {
    /// The machine on offer with `id`.
    #[must_use]
    pub fn find(id: &str) -> Option<&'static Self> {
        MACHINES.iter().find(|machine| machine.id == id)
    }

    /// Its price for each `period`, in pence.
    #[must_use]
    pub const fn price(&self, period: Period) -> u64 {
        match period {
            Period::Day => self.day,
            Period::Week => self.week,
            Period::Month => self.month,
            Period::Quarter => ahead(self.month, 3, period.saving_percent()),
            Period::Year => ahead(self.month, 12, period.saving_percent()),
        }
    }

    /// Its price for the first `period` of a rental, in pence: less than
    /// [`Self::price`] for the first month of a monthly rental.
    #[must_use]
    pub const fn first_price(&self, period: Period) -> u64 {
        match period {
            Period::Month => self.month * FIRST_MONTH_PERCENT / 100,
            _ => self.price(period),
        }
    }
}

/// `months` at `month` pence each, `saving` per cent off, to the nearest
/// pound (half a pound up).
const fn ahead(month: u64, months: u64, saving: u64) -> u64 {
    let pence = month * months * (100 - saving) / 100;
    (pence + 50) / 100 * 100
}

/// What a rental is billed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Period {
    /// A day: the shortest rental Apple's licence allows.
    Day,
    /// A week.
    Week,
    /// A month.
    Month,
    /// Three months, paid ahead.
    Quarter,
    /// Twelve months, paid ahead.
    Year,
}

impl Period {
    /// Every period, in the order the page lists them.
    pub const ALL: [Self; 5] = [
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::Year,
    ];

    /// The period as forms and the database write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::Year => "year",
        }
    }

    /// Reads what [`Self::as_str`] writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|period| period.as_str() == text)
    }

    /// How much less, in per cent, it costs than its months at a month's
    /// price: something only for the periods paid ahead.
    #[must_use]
    pub const fn saving_percent(self) -> u64 {
        match self {
            Self::Quarter => 10,
            Self::Year => 15,
            Self::Day | Self::Week | Self::Month => 0,
        }
    }
}

/// A machine's id: 1 to 32 of `a-z`, `0-9` and `-`, neither beginning nor
/// ending with `-`. What a booking names its machine by, whether or not
/// the machine is still on offer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MachineId(String);

impl MachineId {
    /// Validates `text` as a machine's id.
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

impl fmt::Display for MachineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A chip's name as Apple writes it: 1 to 16 ASCII letters, digits and
/// single spaces between them, such as `M5 Pro`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChipName(String);

impl ChipName {
    /// Validates `text` as a chip's name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let valid = (1..=16).contains(&text.len())
            && text.split(' ').all(|word| {
                !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphanumeric())
            });
        valid.then(|| Self(text.to_owned()))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChipName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a booking asks for, as the offer stood when it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookingTerms {
    /// The machine.
    pub machine: MachineId,
    /// Its chip.
    pub chip: ChipName,
    /// Its memory, in gigabytes.
    pub memory_gb: u16,
    /// Its storage, in gigabytes.
    pub storage_gb: u16,
    /// What the rental is billed by.
    pub period: Period,
    /// Each period's price, in pence.
    pub price: u64,
    /// The first period's price, in pence.
    pub first_price: u64,
}

impl BookingTerms {
    /// The terms on which `machine` is offered for `period`.
    #[must_use]
    pub fn of(machine: &Machine, period: Period) -> Self {
        Self {
            machine: MachineId(machine.id.to_owned()),
            chip: ChipName(machine.chip.to_owned()),
            memory_gb: machine.memory_gb,
            storage_gb: machine.storage_gb,
            period,
            price: machine.price(period),
            first_price: machine.first_price(period),
        }
    }
}

#[cfg(test)]
mod tests;
