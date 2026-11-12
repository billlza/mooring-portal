//! What the pages say of each plan and of an order at each stage.

use portal_core::payment::{OrderId, ReviewReason};
use portal_core::time::UnixTime;

use super::*;

fn catalogue_at(month_price: u64, livemode: bool) -> Catalogue {
    Catalogue::parse(&format!(
        r#"{{"format":"mooring-portal-payments","version":1,"livemode":{livemode},"currency":"gbp",
        "plans":[
          {{"id":"month","kind":"month","days":30,"traffic_gb":100,"amount":{month_price},
            "payment_link":"https://buy.stripe.com/test_month","payment_link_id":"plink_1month"}},
          {{"id":"auto","kind":"month_auto","days":30,"traffic_gb":100,"amount":750,
            "payment_link":"https://buy.stripe.com/test_auto","payment_link_id":"plink_1auto"}},
          {{"id":"pack","kind":"traffic","days":90,"traffic_gb":50,"amount":300,
            "payment_link":"https://buy.stripe.com/test_pack","payment_link_id":"plink_1pack"}}],
        "manage_subscription_url":"https://billing.stripe.com/p/login/test_manage"}}"#
    ))
    .unwrap()
}

fn catalogue() -> Catalogue {
    catalogue_at(800, false)
}

fn zh() -> &'static Strings {
    Lang::Zh.strings()
}

fn user(text: &str) -> MooringUserId {
    MooringUserId::parse(text).unwrap()
}

/// 2026-11-05T12:00:00Z.
const NOW: u64 = 1_793_880_000;

/// An order for `plan`, placed now and standing at `progress`.
fn order(plan: &str, progress: Progress) -> Order {
    let catalogue = catalogue();
    Order {
        id: OrderId::parse("0123456789abcdefghjk").unwrap(),
        user: user("pilotusera"),
        terms: OrderTerms::of(catalogue.plan(plan).unwrap()),
        created_at: UnixTime::from_secs(NOW).unwrap(),
        paid_at: (progress != Progress::Pending).then(|| UnixTime::from_secs(NOW + 3_600).unwrap()),
        progress,
        refunded: 0,
        refunded_in_full: false,
        disputed: false,
        renewal_of: None,
    }
}

#[test]
fn a_plan_is_named_by_its_kind_with_its_terms_and_price() {
    let catalogue = catalogue();
    let listed: Vec<PlanView> = catalogue
        .plans()
        .iter()
        .map(|plan| PlanView::of(plan, false, zh()))
        .collect();
    let plan = |id: &str, name, terms: &str, price: &str| PlanView {
        id: id.to_owned(),
        name,
        terms: terms.to_owned(),
        price: price.to_owned(),
        blocked: false,
    };
    assert_eq!(
        listed,
        [
            plan("month", "月付", "30 天，流量 100 GB", "£8.00"),
            plan(
                "auto",
                "月付 · 自动续费",
                "每 30 天自动续费一次，每期流量 100 GB",
                "£7.50"
            ),
            // A pack counts in the billing cycle Mooring grants it in, and
            // its days are not applied, so none are named.
            plan(
                "pack",
                "流量包",
                "额外流量 50 GB，只在开通它的那个计费周期内有效",
                "£3.00"
            ),
        ]
    );
    // An account that has a renewing plan is not offered another.
    let blocked: Vec<bool> = catalogue
        .plans()
        .iter()
        .map(|plan| PlanView::of(plan, true, zh()).blocked)
        .collect();
    assert_eq!(blocked, [false, true, false]);
    let english = PlanView::of(&catalogue.plans()[2], false, Lang::En.strings());
    assert_eq!(
        (english.name, english.terms.as_str()),
        (
            "Traffic pack",
            "50 GB of extra traffic, only for the billing cycle it is granted in"
        )
    );
}

#[test]
fn an_order_says_where_it_stands_and_what_that_means() {
    let p = &zh().pay;
    for (progress, state, tone, note) in [
        (Progress::Pending, p.state_pending, Tone::Neutral, None),
        (
            Progress::Checking,
            p.state_checking,
            Tone::Warn,
            Some(p.note_checking),
        ),
        (
            Progress::Granted { pulled: false },
            p.state_granted,
            Tone::Good,
            Some(p.note_granted),
        ),
        (
            Progress::Granted { pulled: true },
            p.state_pulled,
            Tone::Good,
            Some(p.note_pulled),
        ),
        (
            Progress::Returned,
            p.state_returned,
            Tone::Neutral,
            Some(p.note_returned),
        ),
    ] {
        let view = OrderView::of(&order("month", progress), zh());
        assert_eq!((view.state, view.tone), (state, tone), "{progress:?}");
        assert_eq!(
            view.notes,
            note.map(str::to_owned).into_iter().collect::<Vec<_>>(),
            "{progress:?}"
        );
    }
    // Whatever it waits for, its owner is told the same: to ask.
    for reason in ReviewReason::ALL {
        let view = OrderView::of(&order("month", Progress::Review(reason)), zh());
        assert_eq!(
            (view.state, view.tone, view.notes),
            (p.state_review, Tone::Warn, vec![p.note_review.to_owned()]),
            "{reason:?}"
        );
    }
    let unpaid = OrderView::of(&order("pack", Progress::Pending), zh());
    assert_eq!(
        unpaid,
        OrderView {
            id: "0123456789abcdefghjk".to_owned(),
            name: "流量包",
            terms: "额外流量 50 GB，只在开通它的那个计费周期内有效".to_owned(),
            price: "£3.00".to_owned(),
            placed: "2026-11-05 20:00".to_owned(),
            paid: None,
            state: "待付款",
            tone: Tone::Neutral,
            notes: Vec::new(),
            renewal: false,
        }
    );
    let paid = OrderView::of(&order("pack", Progress::Checking), zh());
    assert_eq!(paid.paid.as_deref(), Some("2026-11-05 21:00"));
}

#[test]
fn an_order_says_what_has_since_happened_to_the_money() {
    let granted = || order("month", Progress::Granted { pulled: true });
    let notes = |order: &Order| OrderView::of(order, zh()).notes[1..].to_vec();
    assert_eq!(notes(&granted()), Vec::<String>::new());
    let part = Order {
        refunded: 300,
        ..granted()
    };
    assert_eq!(notes(&part), ["已退款 £3.00"]);
    // A granted order refunded in full gives no more from then on (the
    // operator's decision of 2026-11-12); one never granted gave nothing.
    let full = Order {
        refunded: 800,
        refunded_in_full: true,
        ..granted()
    };
    assert_eq!(
        notes(&full),
        ["已全额退款：这张订单开通的时间和流量中，还没用到的部分会收回，已经用掉的不追回。"]
    );
    assert_eq!(
        OrderView::of(&full, Lang::En.strings()).notes[1..],
        [
            "Refunded in full: what this order gave in time and traffic and is not yet used \
             is taken back; what was used is not."
        ]
    );
    let returned = Order {
        refunded: 800,
        refunded_in_full: true,
        ..order("month", Progress::Returned)
    };
    assert_eq!(notes(&returned), ["已全额退款"]);
    let both = Order {
        refunded: 1,
        disputed: true,
        ..granted()
    };
    assert_eq!(
        notes(&both),
        ["已退款 £0.01", "持卡人向银行对这笔付款提出了争议。"]
    );
    let disputed = Order {
        disputed: true,
        ..granted()
    };
    assert_eq!(notes(&disputed), ["持卡人向银行对这笔付款提出了争议。"]);
    let renewal = Order {
        renewal_of: OrderId::parse("zzzzzzzzzzzzzzzzzzzz").ok(),
        ..granted()
    };
    assert!(OrderView::of(&renewal, zh()).renewal);
    assert!(!OrderView::of(&granted(), zh()).renewal);
}

#[test]
fn an_unpaid_order_is_paid_at_its_plans_page_while_its_terms_stand() {
    let bound = user("pilotusera");
    let unpaid = order("month", Progress::Pending);
    let pay = |lang| PayAction::of(&unpaid, &catalogue(), Some(&bound), lang);
    assert_eq!(
        pay(Lang::Zh),
        PayAction::Pay(
            "https://buy.stripe.com/test_month?client_reference_id=0123456789abcdefghjk&locale=zh"
                .to_owned()
        )
    );
    assert_eq!(
        pay(Lang::En),
        PayAction::Pay(
            "https://buy.stripe.com/test_month?client_reference_id=0123456789abcdefghjk&locale=en"
                .to_owned()
        )
    );
    // The catalogue's price for the plan has changed.
    assert_eq!(
        PayAction::of(&unpaid, &catalogue_at(900, false), Some(&bound), Lang::Zh),
        PayAction::Stale
    );
    // The catalogue no longer has the plan.
    let gone = Order {
        terms: OrderTerms {
            plan: portal_core::payment::PlanId::parse("retired").unwrap(),
            ..unpaid.terms.clone()
        },
        ..unpaid.clone()
    };
    assert_eq!(
        PayAction::of(&gone, &catalogue(), Some(&bound), Lang::Zh),
        PayAction::Stale
    );
    // The account is bound to another user now, or to none.
    for binding in [Some(user("pilotuserb")), None] {
        assert_eq!(
            PayAction::of(&unpaid, &catalogue(), binding.as_ref(), Lang::Zh),
            PayAction::Stale,
            "{binding:?}"
        );
    }
    // Once it is paid there is nothing to pay, whatever has changed.
    for progress in [
        Progress::Checking,
        Progress::Granted { pulled: false },
        Progress::Review(ReviewReason::Amount),
        Progress::Returned,
    ] {
        let paid = order("month", progress);
        assert_eq!(
            PayAction::of(&paid, &catalogue(), Some(&bound), Lang::Zh),
            PayAction::None
        );
        assert_eq!(
            PayAction::of(&paid, &catalogue_at(900, false), None, Lang::Zh),
            PayAction::None
        );
    }
}

#[test]
fn the_shop_says_whether_a_plan_renews_and_where_to_stop_it() {
    let p = &zh().pay;
    let renewal = |state| AutoRenewal {
        plan: catalogue().plans()[1].id.clone(),
        state,
    };
    let orders = [order("month", Progress::Checking)];
    let none = ShopView::of(&catalogue(), &orders, &[], zh());
    assert_eq!((none.renewal, none.manage_url.as_deref()), (None, None));
    assert!(none.plans.iter().all(|plan| !plan.blocked));
    assert_eq!(none.orders, [OrderView::of(&orders[0], zh())]);
    assert!(none.test_mode);
    assert!(!ShopView::of(&catalogue_at(800, true), &[], &[], zh()).test_mode);
    let manage = Some("https://billing.stripe.com/p/login/test_manage");
    for (renewals, said) in [
        (vec![renewal(Renewal::Active)], p.renewing_active),
        (vec![renewal(Renewal::Ending)], p.renewing_ending),
        // While any of them runs, Stripe will charge again.
        (
            vec![renewal(Renewal::Ending), renewal(Renewal::Active)],
            p.renewing_active,
        ),
        (
            vec![renewal(Renewal::Ending), renewal(Renewal::Ending)],
            p.renewing_ending,
        ),
    ] {
        let shop = ShopView::of(&catalogue(), &[], &renewals, zh());
        assert_eq!(shop.renewal, Some(said), "{renewals:?}");
        assert_eq!(shop.manage_url.as_deref(), manage);
        assert_eq!(
            shop.plans
                .iter()
                .map(|plan| plan.blocked)
                .collect::<Vec<_>>(),
            [false, true, false]
        );
        assert!(shop.orders.is_empty());
    }
}
