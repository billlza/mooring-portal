//! The shape of a language's text, page by page.

/// Every phrase of the portal in one language.
#[derive(Debug)]
pub(crate) struct Strings {
    pub(crate) common: Common,
    pub(crate) nav: Nav,
    pub(crate) home: Home,
    pub(crate) download: Download,
    pub(crate) plans: Plans,
    pub(crate) help: Help,
    pub(crate) auth: Auth,
    pub(crate) account: AccountText,
    pub(crate) subscription: SubscriptionText,
    pub(crate) cfm: CfmText,
    pub(crate) pay: PayText,
    pub(crate) rental: RentalText,
}

/// Text on every page.
#[derive(Debug)]
pub(crate) struct Common {
    pub(crate) skip: &'static str,
    pub(crate) nav_label: &'static str,
    pub(crate) menu: &'static str,
    pub(crate) switch_language: &'static str,
    pub(crate) switch_language_short: &'static str,
    pub(crate) sign_in: &'static str,
    pub(crate) my_account: &'static str,
    /// The footer's link to the help page.
    pub(crate) help: &'static str,
    pub(crate) beijing_time: &'static str,
}

/// The navigation.
#[derive(Debug)]
pub(crate) struct Nav {
    pub(crate) home: &'static str,
    pub(crate) download: &'static str,
    pub(crate) plans: &'static str,
    /// The Mac rental page.
    pub(crate) mac: &'static str,
    pub(crate) help: &'static str,
}

/// A titled paragraph.
#[derive(Debug)]
pub(crate) struct Item {
    pub(crate) title: &'static str,
    pub(crate) body: &'static str,
}

/// The home page.
#[derive(Debug)]
pub(crate) struct Home {
    pub(crate) title: &'static str,
    /// The line above the headline: the service, and what it is.
    pub(crate) eyebrow: &'static str,
    /// What stands under the headline, a line to each part.
    pub(crate) lead: [&'static str; 2],
    /// The two buttons: to the plans, and to the download.
    pub(crate) plans: &'static str,
    pub(crate) download: &'static str,
    /// Two facts about the service, set side by side under the buttons.
    pub(crate) facts: [&'static str; 2],
    /// What the three steps are, for those who cannot see them set out.
    pub(crate) steps_label: &'static str,
    /// The steps, in the order the home page draws their icons: a plan,
    /// the import, and use.
    pub(crate) steps: [Item; 3],
}

/// The download page.
#[derive(Debug)]
pub(crate) struct Download {
    pub(crate) title: &'static str,
    pub(crate) lead: &'static str,
    pub(crate) mac_title: &'static str,
    pub(crate) mac_requirements: &'static str,
    pub(crate) version: &'static str,
    pub(crate) published: &'static str,
    pub(crate) button: &'static str,
    pub(crate) release_page: &'static str,
    pub(crate) checksum: &'static str,
    pub(crate) checksum_hint: &'static str,
    pub(crate) signed: &'static str,
    pub(crate) fallback: &'static str,
    /// No release has been imported yet.
    pub(crate) not_yet: &'static str,
    /// When the release was last found to be the latest; takes the time.
    pub(crate) checked: &'static str,
    /// The release has not been checked for a day.
    pub(crate) stale: &'static str,
    pub(crate) windows_title: &'static str,
    pub(crate) windows_status: &'static str,
    pub(crate) windows_body: &'static str,
    pub(crate) unsupported: &'static str,
    pub(crate) setup_title: &'static str,
    pub(crate) setup_steps: [&'static str; 3],
}

/// The page on how subscriptions work.
#[derive(Debug)]
pub(crate) struct Plans {
    pub(crate) title: &'static str,
    pub(crate) lead: &'static str,
    /// What the service is.
    pub(crate) about_title: &'static str,
    pub(crate) about_body: &'static str,
    /// What it promises: steady, clear to see, discreet, mainland sites
    /// direct.
    pub(crate) promises: [Item; 4],
    pub(crate) how_title: &'static str,
    pub(crate) how_steps: [&'static str; 4],
    pub(crate) sections: [Item; 4],
    /// How one pays: while the portal sells nothing, and once it does.
    pub(crate) payment_closed: Item,
    pub(crate) payment_open: Item,
    /// What it is not.
    pub(crate) note: &'static str,
}

/// The help page.
#[derive(Debug)]
pub(crate) struct Help {
    pub(crate) title: &'static str,
    pub(crate) contact_title: &'static str,
    pub(crate) contact_body: &'static str,
    pub(crate) faq_title: &'static str,
    pub(crate) faqs: [Item; 6],
}

/// Logging in, registering and their errors.
#[derive(Debug)]
pub(crate) struct Auth {
    pub(crate) login_title: &'static str,
    pub(crate) register_title: &'static str,
    pub(crate) email: &'static str,
    pub(crate) password: &'static str,
    pub(crate) confirm: &'static str,
    pub(crate) login_submit: &'static str,
    pub(crate) register_submit: &'static str,
    pub(crate) to_register: &'static str,
    pub(crate) to_login: &'static str,
    pub(crate) password_hint: &'static str,
    pub(crate) register_note: &'static str,
    pub(crate) logout: &'static str,
    pub(crate) invalid_email: &'static str,
    pub(crate) password_too_short: &'static str,
    pub(crate) password_too_long: &'static str,
    pub(crate) password_control: &'static str,
    pub(crate) password_is_email: &'static str,
    pub(crate) confirm_mismatch: &'static str,
    pub(crate) email_taken: &'static str,
    pub(crate) registration_full: &'static str,
    pub(crate) login_failed: &'static str,
    pub(crate) too_many_attempts: &'static str,
    pub(crate) busy: &'static str,
}

/// The account pages.
#[derive(Debug)]
pub(crate) struct AccountText {
    pub(crate) title: &'static str,
    pub(crate) tab_account: &'static str,
    pub(crate) tab_subscription: &'static str,
    pub(crate) code: &'static str,
    pub(crate) code_hint: &'static str,
    pub(crate) email: &'static str,
    pub(crate) created: &'static str,
    pub(crate) binding: &'static str,
    pub(crate) bound: &'static str,
    pub(crate) unbound: &'static str,
    pub(crate) bound_since: &'static str,
    pub(crate) unbound_hint: &'static str,
    pub(crate) security: &'static str,
    pub(crate) change_password: &'static str,
    pub(crate) password_title: &'static str,
    pub(crate) current_password: &'static str,
    pub(crate) new_password: &'static str,
    pub(crate) password_submit: &'static str,
    pub(crate) password_note: &'static str,
    pub(crate) current_password_wrong: &'static str,
    pub(crate) welcome: &'static str,
    pub(crate) password_changed: &'static str,
    pub(crate) password_changed_link_revoked: &'static str,
}

/// The subscription page.
#[derive(Debug)]
pub(crate) struct SubscriptionText {
    pub(crate) title: &'static str,
    pub(crate) none_title: &'static str,
    pub(crate) none_body: &'static str,
    pub(crate) awaiting_title: &'static str,
    pub(crate) awaiting_body: &'static str,
    pub(crate) status: &'static str,
    pub(crate) active: &'static str,
    pub(crate) suspended: &'static str,
    pub(crate) revoked: &'static str,
    pub(crate) reason_operator: &'static str,
    pub(crate) reason_quota: &'static str,
    pub(crate) reason_quota_earlier: &'static str,
    pub(crate) reason_expired: &'static str,
    pub(crate) reason_abuse: &'static str,
    pub(crate) revoked_body: &'static str,
    pub(crate) since: &'static str,
    pub(crate) cycle: &'static str,
    pub(crate) cycle_ended_label: &'static str,
    pub(crate) resets: &'static str,
    pub(crate) ended: &'static str,
    pub(crate) usage: &'static str,
    pub(crate) usage_ended: &'static str,
    pub(crate) usage_unknown: &'static str,
    pub(crate) usage_unknown_hint: &'static str,
    pub(crate) counted_through: &'static str,
    pub(crate) no_report: &'static str,
    pub(crate) quota: &'static str,
    pub(crate) quota_none: &'static str,
    pub(crate) quota_none_hint: &'static str,
    pub(crate) level_within: &'static str,
    pub(crate) level_near: &'static str,
    pub(crate) level_over: &'static str,
    pub(crate) level_unknown: &'static str,
    pub(crate) quota_used: &'static str,
    pub(crate) action_suspend: &'static str,
    pub(crate) action_warn: &'static str,
    pub(crate) expiry: &'static str,
    pub(crate) expiry_never: &'static str,
    pub(crate) days_left_one: &'static str,
    pub(crate) days_left_many: &'static str,
    pub(crate) expired: &'static str,
    pub(crate) as_of: &'static str,
    pub(crate) stale: &'static str,
    pub(crate) cycle_ended: &'static str,
    pub(crate) lag_note: &'static str,
    pub(crate) data_note: &'static str,
}

/// Getting the CFM profile: the subscription page's panel and the page
/// that shows a new subscription link.
#[derive(Debug)]
pub(crate) struct CfmText {
    pub(crate) title: &'static str,
    pub(crate) intro: &'static str,
    pub(crate) waiting: &'static str,
    pub(crate) preparing: &'static str,
    pub(crate) generated: &'static str,
    pub(crate) update_hint: &'static str,
    pub(crate) link_title: &'static str,
    pub(crate) link_none: &'static str,
    pub(crate) link_created: &'static str,
    pub(crate) link_paused: &'static str,
    pub(crate) link_create: &'static str,
    pub(crate) link_rotate: &'static str,
    pub(crate) link_revoke: &'static str,
    pub(crate) link_revoked: &'static str,
    pub(crate) link_unavailable: &'static str,
    pub(crate) file_title: &'static str,
    pub(crate) file_body: &'static str,
    pub(crate) download: &'static str,
    pub(crate) made_title: &'static str,
    pub(crate) made_lead: &'static str,
    pub(crate) made_label: &'static str,
    pub(crate) made_warning: &'static str,
    pub(crate) back: &'static str,
}

/// Plans, orders and payment: the plans page's price list, the
/// subscription page's shop, and an order's page.
#[derive(Debug)]
pub(crate) struct PayText {
    pub(crate) plans_title: &'static str,
    pub(crate) plans_lead: &'static str,
    pub(crate) kind_month: &'static str,
    pub(crate) kind_month_auto: &'static str,
    pub(crate) kind_traffic: &'static str,
    /// What each kind of plan buys; each takes the days, then the traffic.
    pub(crate) terms_month: &'static str,
    pub(crate) terms_month_auto: &'static str,
    pub(crate) terms_traffic: &'static str,
    pub(crate) to_buy: &'static str,
    /// The catalogue's payment pages take no real payments.
    pub(crate) test_mode: &'static str,
    pub(crate) buy_title: &'static str,
    pub(crate) buy_intro: &'static str,
    pub(crate) buy: &'static str,
    pub(crate) renewing_active: &'static str,
    pub(crate) renewing_ending: &'static str,
    pub(crate) manage: &'static str,
    pub(crate) manage_hint: &'static str,
    /// Why a renewing plan cannot be ordered now.
    pub(crate) auto_blocked: &'static str,
    pub(crate) orders_title: &'static str,
    pub(crate) order_view: &'static str,
    pub(crate) order_title: &'static str,
    pub(crate) order_id: &'static str,
    pub(crate) order_plan: &'static str,
    pub(crate) order_price: &'static str,
    pub(crate) order_placed: &'static str,
    pub(crate) order_paid: &'static str,
    pub(crate) order_state: &'static str,
    pub(crate) pay: &'static str,
    pub(crate) pay_hint: &'static str,
    /// The order's terms are no longer the catalogue's.
    pub(crate) pay_stale: &'static str,
    pub(crate) renewal_note: &'static str,
    pub(crate) state_pending: &'static str,
    pub(crate) state_checking: &'static str,
    pub(crate) state_granted: &'static str,
    pub(crate) state_pulled: &'static str,
    pub(crate) state_review: &'static str,
    pub(crate) state_returned: &'static str,
    pub(crate) note_checking: &'static str,
    pub(crate) note_granted: &'static str,
    pub(crate) note_pulled: &'static str,
    pub(crate) note_review: &'static str,
    pub(crate) note_returned: &'static str,
    /// Takes the amount refunded.
    pub(crate) note_refunded_part: &'static str,
    pub(crate) note_refunded_full: &'static str,
    /// A granted order refunded in full: Mooring takes back what it gave
    /// and is not yet used.
    pub(crate) note_taken_back: &'static str,
    pub(crate) note_disputed: &'static str,
    pub(crate) notice_unbound: &'static str,
    pub(crate) notice_plan_gone: &'static str,
}

/// The Mac rental page: the machines on offer, how to pay for them, and
/// the signed-in account's bookings.
#[derive(Debug)]
pub(crate) struct RentalText {
    pub(crate) title: &'static str,
    pub(crate) lead: &'static str,
    /// The page's three steps: a machine, a period, the booking.
    pub(crate) machine_step: &'static str,
    pub(crate) period_step: &'static str,
    pub(crate) book_step: &'static str,
    /// A machine's chip; each takes a count of cores.
    pub(crate) cpu: &'static str,
    pub(crate) gpu: &'static str,
    /// Its memory and storage; each takes gigabytes.
    pub(crate) memory: &'static str,
    pub(crate) storage: &'static str,
    /// Its Ethernet port; takes the speed, such as `2.5Gb`.
    pub(crate) ethernet: &'static str,
    /// The periods a rental is billed by.
    pub(crate) day: &'static str,
    pub(crate) week: &'static str,
    pub(crate) month: &'static str,
    pub(crate) quarter: &'static str,
    pub(crate) year: &'static str,
    /// What a price is for, after it: one of each period.
    pub(crate) per_day: &'static str,
    pub(crate) per_week: &'static str,
    pub(crate) per_month: &'static str,
    pub(crate) per_quarter: &'static str,
    pub(crate) per_year: &'static str,
    /// What paying ahead saves; takes the percentage.
    pub(crate) saving: &'static str,
    /// What the first month of a monthly rental saves; takes the
    /// percentage.
    pub(crate) first_month_saving: &'static str,
    /// The first month's price; takes it.
    pub(crate) first_month: &'static str,
    pub(crate) period_hint: &'static str,
    /// What happens once a machine is booked, and on what terms.
    pub(crate) terms: [&'static str; 3],
    pub(crate) sign_in_first: &'static str,
    pub(crate) book: &'static str,
    pub(crate) bookings_title: &'static str,
    /// When a booking was made; takes the time.
    pub(crate) placed: &'static str,
    pub(crate) withdraw: &'static str,
    pub(crate) one_each: &'static str,
    pub(crate) notice_booked: &'static str,
    pub(crate) notice_withdrawn: &'static str,
    /// The machine booked is no longer on offer.
    pub(crate) notice_gone: &'static str,
}
