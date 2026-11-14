use super::*;

/// Each machine's prices in pence for a day, a week, a month, a quarter
/// and a year, and the first month's: the figures the operator was shown
/// on 2026-11-14.
const PRICES: [(&str, [u64; 5], u64); 4] = [
    ("m4-16-256", [1_000, 3_500, 7_900, 21_300, 80_600], 4_740),
    ("m6-16-256", [1_500, 5_200, 11_900, 32_100, 121_400], 7_140),
    ("m6-24-512", [2_200, 7_400, 16_900, 45_600, 172_400], 10_140),
    (
        "m5pro-24-512",
        [2_800, 9_600, 21_900, 59_100, 223_400],
        13_140,
    ),
];

#[test]
fn every_machine_has_the_prices_the_operator_was_shown() {
    assert_eq!(MACHINES.len(), PRICES.len());
    for (machine, (id, prices, first_month)) in MACHINES.iter().zip(PRICES) {
        assert_eq!(machine.id, id);
        for (period, price) in Period::ALL.into_iter().zip(prices) {
            assert_eq!(machine.price(period), price, "{id} {period:?}");
        }
        assert_eq!(machine.first_price(Period::Month), first_month, "{id}");
        for period in [Period::Day, Period::Week, Period::Quarter, Period::Year] {
            assert_eq!(
                machine.first_price(period),
                machine.price(period),
                "{id} {period:?}"
            );
        }
    }
}

#[test]
fn paying_ahead_saves_what_the_page_says_to_the_pound() {
    for machine in &MACHINES {
        for (period, months) in [(Period::Quarter, 3), (Period::Year, 12)] {
            let full = machine.price(Period::Month) * months;
            let asked = machine.price(period);
            // Whole pounds, and within half a pound of the saving.
            assert_eq!(asked % 100, 0, "{} {period:?}", machine.id);
            let exact = full * (100 - period.saving_percent());
            assert!(
                (asked * 100).abs_diff(exact) <= 50 * 100,
                "{} {period:?}: {asked} against {exact}",
                machine.id
            );
        }
    }
    assert_eq!(ahead(7_900, 3, 10), 21_300);
    // Half a pound goes up.
    assert_eq!(ahead(1_000, 1, 95), 100);
    assert_eq!(ahead(1_100, 1, 95), 100);
    assert_eq!(ahead(1_000, 1, 0), 1_000);
}

#[test]
fn only_the_periods_paid_ahead_save() {
    assert_eq!(Period::ALL.map(Period::saving_percent), [0, 0, 0, 10, 15]);
    assert_eq!(FIRST_MONTH_PERCENT, 60);
}

#[test]
fn machines_are_found_by_their_ids_and_described_as_apple_sells_them() {
    let ids: Vec<&str> = MACHINES.iter().map(|machine| machine.id).collect();
    assert_eq!(ids, ["m4-16-256", "m6-16-256", "m6-24-512", "m5pro-24-512"]);
    for machine in &MACHINES {
        assert_eq!(Machine::find(machine.id), Some(machine));
        // Every id on offer is one a booking can keep.
        assert_eq!(MachineId::parse(machine.id).unwrap().as_str(), machine.id);
        assert_eq!(
            ChipName::parse(machine.chip).unwrap().as_str(),
            machine.chip
        );
    }
    assert_eq!(Machine::find("m6"), None);
    assert_eq!(Machine::find(""), None);
    let m5 = Machine::find("m5pro-24-512").unwrap();
    assert_eq!(
        (
            m5.chip,
            m5.cpu_cores,
            m5.gpu_cores,
            m5.memory_gb,
            m5.storage_gb,
            m5.ethernet_mbps
        ),
        ("M5 Pro", 15, 16, 24, 512, 2_500)
    );
    let m4 = Machine::find("m4-16-256").unwrap();
    assert_eq!(
        (m4.cpu_cores, m4.gpu_cores, m4.ethernet_mbps),
        (10, 10, 1_000)
    );
}

#[test]
fn periods_round_trip_and_nothing_else_parses() {
    for period in Period::ALL {
        assert_eq!(Period::parse(period.as_str()), Some(period));
    }
    assert_eq!(
        Period::ALL.map(Period::as_str),
        ["day", "week", "month", "quarter", "year"]
    );
    for bad in ["", "Month", "hour", "months", " month"] {
        assert_eq!(Period::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn machine_ids_and_chip_names_are_checked() {
    for good in ["m6", "m5pro-24-512", "a", "0", &"a".repeat(32)] {
        let id = MachineId::parse(good).unwrap();
        assert_eq!(id.to_string(), good);
    }
    for bad in [
        "",
        "-m6",
        "m6-",
        "M6",
        "m6 24",
        "m6_24",
        "m6é",
        &"a".repeat(33),
    ] {
        assert_eq!(MachineId::parse(bad), None, "{bad:?}");
    }
    for good in ["M6", "M5 Pro", "M4 Max Ultra", &"M".repeat(16)] {
        let chip = ChipName::parse(good).unwrap();
        assert_eq!(chip.to_string(), good);
    }
    for bad in [
        "",
        " M6",
        "M6 ",
        "M5  Pro",
        "M6-Pro",
        "M6\n",
        "芯片",
        &"M".repeat(17),
    ] {
        assert_eq!(ChipName::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn a_booking_keeps_the_terms_its_machine_was_offered_on() {
    let machine = Machine::find("m6-24-512").unwrap();
    let terms = BookingTerms::of(machine, Period::Month);
    assert_eq!(
        terms,
        BookingTerms {
            machine: MachineId::parse("m6-24-512").unwrap(),
            chip: ChipName::parse("M6").unwrap(),
            memory_gb: 24,
            storage_gb: 512,
            period: Period::Month,
            price: 16_900,
            first_price: 10_140,
        }
    );
    let yearly = BookingTerms::of(machine, Period::Year);
    assert_eq!((yearly.price, yearly.first_price), (172_400, 172_400));
}
