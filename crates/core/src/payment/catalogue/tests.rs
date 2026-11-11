//! The catalogue: what a valid one holds, and that each thing wrong with
//! one is refused by name.

use super::*;

/// A plan as the operator writes one, with `changes` made to its keys.
fn plan(id: &str, kind: &str, link: &str, changes: &[(&str, &str)]) -> String {
    let mut keys = vec![
        ("id", format!("\"{id}\"")),
        ("kind", format!("\"{kind}\"")),
        ("days", "30".to_owned()),
        ("traffic_gb", "100".to_owned()),
        ("amount", "800".to_owned()),
        (
            "payment_link",
            format!("\"https://buy.stripe.com/test_{link}\""),
        ),
        ("payment_link_id", format!("\"plink_1{link}\"")),
    ];
    for (key, value) in changes {
        match keys.iter_mut().find(|(name, _)| name == key) {
            Some(slot) => slot.1 = (*value).to_owned(),
            None => keys.push((key, (*value).to_owned())),
        }
    }
    let keys: Vec<String> = keys
        .iter()
        .map(|(key, value)| format!("\"{key}\": {value}"))
        .collect();
    format!("{{{}}}", keys.join(", "))
}

/// A catalogue of `plans`, with `changes` made to its own keys; a change
/// to nothing (`""`) leaves the key out.
fn catalogue(plans: &[String], changes: &[(&str, &str)]) -> String {
    let mut keys = vec![
        ("format", "\"mooring-portal-payments\"".to_owned()),
        ("version", "1".to_owned()),
        ("livemode", "false".to_owned()),
        ("currency", "\"gbp\"".to_owned()),
        ("plans", format!("[{}]", plans.join(", "))),
        (
            "manage_subscription_url",
            "\"https://billing.stripe.com/p/login/test_4gw6oJchs69w47e7ss\"".to_owned(),
        ),
    ];
    for (key, value) in changes {
        match keys.iter_mut().find(|(name, _)| name == key) {
            Some(slot) => slot.1 = (*value).to_owned(),
            None => keys.push((key, (*value).to_owned())),
        }
    }
    let keys: Vec<String> = keys
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| format!("\"{key}\": {value}"))
        .collect();
    format!("{{{}}}", keys.join(", "))
}

fn three() -> Vec<String> {
    vec![
        plan("month-100", "month", "aaa", &[]),
        plan("month-100-auto", "month_auto", "bbb", &[]),
        plan(
            "pack-50",
            "traffic",
            "ccc",
            &[("traffic_gb", "50"), ("amount", "400"), ("days", "45")],
        ),
    ]
}

#[test]
fn a_catalogue_holds_its_plans_in_the_operators_order() {
    let catalogue = Catalogue::parse(&catalogue(&three(), &[])).unwrap();
    assert!(!catalogue.livemode());
    assert_eq!(
        catalogue.manage_url(),
        Some("https://billing.stripe.com/p/login/test_4gw6oJchs69w47e7ss")
    );
    let ids: Vec<&str> = catalogue
        .plans()
        .iter()
        .map(|plan| plan.id.as_str())
        .collect();
    assert_eq!(ids, ["month-100", "month-100-auto", "pack-50"]);
    let pack = catalogue.plan("pack-50").unwrap();
    assert_eq!(
        *pack,
        Plan {
            id: PlanId::parse("pack-50").unwrap(),
            kind: PlanKind::Traffic,
            days: 45,
            // Gigabytes are decimal, as quotas are set.
            traffic_bytes: 50_000_000_000,
            amount: 400,
            currency: Currency::parse("gbp").unwrap(),
            payment_link: "https://buy.stripe.com/test_ccc".to_owned(),
            payment_link_id: "plink_1ccc".to_owned(),
        }
    );
    assert_eq!(catalogue.plan("month-100").unwrap().kind, PlanKind::Month);
    assert!(catalogue.plan("month-1000").is_none());
    assert_eq!(pack.id.to_string(), "pack-50");
}

#[test]
fn a_catalogue_says_whether_its_pages_take_real_payments() {
    let live = Catalogue::parse(&catalogue(&three(), &[("livemode", "true")])).unwrap();
    assert!(live.livemode());
}

#[test]
fn without_a_renewing_plan_no_page_for_managing_one_is_needed() {
    let plans = [plan("month", "month", "aaa", &[])];
    let parsed = Catalogue::parse(&catalogue(&plans, &[("manage_subscription_url", "")])).unwrap();
    assert_eq!(parsed.manage_url(), None);
    // With one, it is.
    assert_eq!(
        Catalogue::parse(&catalogue(&three(), &[("manage_subscription_url", "")])),
        Err(CatalogueError::NoManageUrl)
    );
    assert_eq!(
        Catalogue::parse(&catalogue(&three(), &[("manage_subscription_url", "null")])),
        Err(CatalogueError::NoManageUrl)
    );
}

#[test]
fn the_extremes_of_each_figure_are_accepted() {
    for changes in [
        [("days", "1"), ("traffic_gb", "1"), ("amount", "1")],
        [
            ("days", "3660"),
            ("traffic_gb", "100000"),
            ("amount", "10000000"),
        ],
    ] {
        let plans = [plan("a", "traffic", "aaa", &changes)];
        let parsed = Catalogue::parse(&catalogue(&plans, &[])).unwrap();
        assert_eq!(parsed.plans().len(), 1);
    }
    let most = Catalogue::parse(&catalogue(
        &[plan("a", "month", "aaa", &[("traffic_gb", "100000")])],
        &[],
    ))
    .unwrap();
    assert_eq!(most.plans()[0].traffic_bytes, 100_000_000_000_000);
    // Twenty-four plans are the most.
    let many: Vec<String> = (0..24)
        .map(|n| plan(&format!("p{n}"), "month", &format!("x{n}"), &[]))
        .collect();
    assert_eq!(
        Catalogue::parse(&catalogue(&many, &[]))
            .unwrap()
            .plans()
            .len(),
        24
    );
    let too_many: Vec<String> = (0..25)
        .map(|n| plan(&format!("p{n}"), "month", &format!("x{n}"), &[]))
        .collect();
    assert_eq!(
        Catalogue::parse(&catalogue(&too_many, &[])),
        Err(CatalogueError::PlanCount)
    );
}

#[test]
fn every_currency_with_hundredths_is_read_in_lowercase() {
    for code in ["aud", "cad", "cny", "eur", "gbp", "hkd", "sgd", "usd"] {
        let parsed = Catalogue::parse(&catalogue(
            &three(),
            &[("currency", &format!("\"{code}\""))],
        ))
        .unwrap();
        assert_eq!(parsed.plans()[0].currency.as_str(), code);
    }
    for bad in ["GBP", "jpy", "gb", "", "gbpp"] {
        assert_eq!(
            Catalogue::parse(&catalogue(&three(), &[("currency", &format!("\"{bad}\""))])),
            Err(CatalogueError::Currency),
            "{bad}"
        );
    }
}

#[test]
fn a_catalogue_of_another_shape_is_refused() {
    let valid = catalogue(&three(), &[]);
    for (changes, error) in [
        (
            [("format", "\"mooring-portal-plans\"")],
            CatalogueError::Format,
        ),
        ([("version", "2")], CatalogueError::Format),
        ([("plans", "[]")], CatalogueError::PlanCount),
        (
            [(
                "manage_subscription_url",
                "\"https://billing.stripe.com/p/login/\"",
            )],
            CatalogueError::ManageUrl,
        ),
        (
            [(
                "manage_subscription_url",
                "\"https://billing.stripe.com/p/login/a?prefilled_email=b\"",
            )],
            CatalogueError::ManageUrl,
        ),
        (
            [(
                "manage_subscription_url",
                "\"https://example.com/p/login/a\"",
            )],
            CatalogueError::ManageUrl,
        ),
        (
            [("extra", "1")],
            CatalogueError::Malformed(Malformation::Shape),
        ),
        (
            [("livemode", "\"no\"")],
            CatalogueError::Malformed(Malformation::Shape),
        ),
    ] {
        assert_eq!(
            Catalogue::parse(&catalogue(&three(), &changes)),
            Err(error),
            "{changes:?}"
        );
    }
    assert_eq!(
        Catalogue::parse(&valid[..valid.len() - 1]),
        Err(CatalogueError::Malformed(Malformation::Truncated))
    );
    assert_eq!(
        Catalogue::parse("plans"),
        Err(CatalogueError::Malformed(Malformation::Syntax))
    );
    let large = format!("{valid}{}", " ".repeat(MAX_CATALOGUE_BYTES));
    assert_eq!(Catalogue::parse(&large), Err(CatalogueError::TooLarge));
    let largest = format!("{valid}{}", " ".repeat(MAX_CATALOGUE_BYTES - valid.len()));
    assert!(Catalogue::parse(&largest).is_ok());
}

#[test]
fn a_plan_that_is_wrong_is_refused_by_name() {
    let name = || "month-100".to_owned();
    for (changes, error) in [
        (vec![("kind", "\"year\"")], CatalogueError::Kind(name())),
        (vec![("days", "0")], CatalogueError::Days(name())),
        (vec![("days", "3661")], CatalogueError::Days(name())),
        (vec![("traffic_gb", "0")], CatalogueError::Traffic(name())),
        (
            vec![("traffic_gb", "100001")],
            CatalogueError::Traffic(name()),
        ),
        (vec![("amount", "0")], CatalogueError::Amount(name())),
        (vec![("amount", "10000001")], CatalogueError::Amount(name())),
        (
            vec![("payment_link", "\"http://buy.stripe.com/test_aaa\"")],
            CatalogueError::PaymentLink(name()),
        ),
        (
            vec![(
                "payment_link",
                "\"https://buy.stripe.com.example/test_aaa\"",
            )],
            CatalogueError::PaymentLink(name()),
        ),
        (
            vec![("payment_link", "\"https://buy.stripe.com/\"")],
            CatalogueError::PaymentLink(name()),
        ),
        (
            vec![(
                "payment_link",
                "\"https://buy.stripe.com/test_aaa?client_reference_id=x\"",
            )],
            CatalogueError::PaymentLink(name()),
        ),
        (
            vec![("payment_link", "\"https://buy.stripe.com/a/b\"")],
            CatalogueError::PaymentLink(name()),
        ),
        (
            vec![("payment_link_id", "\"plink_\"")],
            CatalogueError::PaymentLinkId(name()),
        ),
        (
            vec![("payment_link_id", "\"price_1aaa\"")],
            CatalogueError::PaymentLinkId(name()),
        ),
        (
            vec![("payment_link_id", "\"plink_1_a\"")],
            CatalogueError::PaymentLinkId(name()),
        ),
        (
            vec![("days", "\"30\"")],
            CatalogueError::Malformed(Malformation::Shape),
        ),
        (
            vec![("amount", "-1")],
            CatalogueError::Malformed(Malformation::Shape),
        ),
        (
            vec![("note", "\"x\"")],
            CatalogueError::Malformed(Malformation::Shape),
        ),
    ] {
        let plans = [plan("month-100", "month", "aaa", &changes)];
        assert_eq!(
            Catalogue::parse(&catalogue(&plans, &[])),
            Err(error),
            "{changes:?}"
        );
    }
    // A page's word may be 128 long and no longer.
    for (length, accepted) in [(128, true), (129, false)] {
        let link = format!("\"https://buy.stripe.com/{}\"", "a".repeat(length));
        let plans = [plan(
            "month-100",
            "month",
            "aaa",
            &[("payment_link", &link)],
        )];
        assert_eq!(
            Catalogue::parse(&catalogue(&plans, &[])).is_ok(),
            accepted,
            "{length}"
        );
    }
}

#[test]
fn plan_ids_are_short_lowercase_words() {
    for good in ["a", "month-100", "0", &"a".repeat(32)] {
        assert_eq!(PlanId::parse(good).unwrap().as_str(), good);
    }
    for bad in [
        "",
        "-a",
        "a-",
        "Month",
        "a b",
        "a_b",
        &"a".repeat(33),
        "\u{e9}",
    ] {
        assert_eq!(PlanId::parse(bad), None, "{bad:?}");
    }
    // The catalogue says which plan it was, counting from one.
    let plans = [
        plan("good", "month", "aaa", &[]),
        plan("Bad", "month", "bbb", &[]),
    ];
    assert_eq!(
        Catalogue::parse(&catalogue(&plans, &[])),
        Err(CatalogueError::PlanId(2))
    );
}

#[test]
fn two_plans_share_neither_an_id_nor_a_payment_page() {
    let twice = [
        plan("same", "month", "aaa", &[]),
        plan("same", "traffic", "bbb", &[]),
    ];
    assert_eq!(
        Catalogue::parse(&catalogue(&twice, &[])),
        Err(CatalogueError::DuplicatePlan("same".to_owned()))
    );
    let one_page = [
        plan("first", "month", "aaa", &[]),
        plan(
            "second",
            "traffic",
            "bbb",
            &[("payment_link", "\"https://buy.stripe.com/test_aaa\"")],
        ),
    ];
    assert_eq!(
        Catalogue::parse(&catalogue(&one_page, &[])),
        Err(CatalogueError::SharedLink("second".to_owned()))
    );
    let one_id = [
        plan("first", "month", "aaa", &[]),
        plan(
            "second",
            "traffic",
            "bbb",
            &[("payment_link_id", "\"plink_1aaa\"")],
        ),
    ];
    assert_eq!(
        Catalogue::parse(&catalogue(&one_id, &[])),
        Err(CatalogueError::SharedLink("second".to_owned()))
    );
}

#[test]
fn kinds_are_written_as_the_catalogue_writes_them() {
    for (kind, text, renews) in [
        (PlanKind::Month, "month", false),
        (PlanKind::MonthAuto, "month_auto", true),
        (PlanKind::Traffic, "traffic", false),
    ] {
        assert_eq!(kind.as_str(), text);
        assert_eq!(PlanKind::parse(text), Some(kind));
        assert_eq!(kind.renews(), renews);
    }
    assert_eq!(PlanKind::parse("Month"), None);
    assert_eq!(PlanKind::ALL.len(), 3);
}

#[test]
fn refusals_say_what_to_write() {
    for (error, text) in [
        (
            CatalogueError::Format,
            "the catalogue is not \"mooring-portal-payments\" version 1",
        ),
        (
            CatalogueError::PlanCount,
            "the catalogue must hold between 1 and 24 plans",
        ),
        (
            CatalogueError::Days("m".to_owned()),
            "plan \"m\": its days must be between 1 and 3660",
        ),
        (
            CatalogueError::Amount("m".to_owned()),
            "plan \"m\": its amount must be between 1 and 10000000 hundredths",
        ),
        (
            CatalogueError::PaymentLink("m".to_owned()),
            "plan \"m\": its payment_link must be https://buy.stripe.com/ and letters, digits or _",
        ),
        (
            CatalogueError::ManageUrl,
            "manage_subscription_url must be https://billing.stripe.com/p/login/ and letters, \
             digits or _",
        ),
        (
            CatalogueError::TooLarge,
            "the catalogue is larger than 65536 bytes",
        ),
    ] {
        assert_eq!(error.to_string(), text);
    }
}
