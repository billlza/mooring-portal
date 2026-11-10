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
    pub(crate) help: &'static str,
    pub(crate) footer_note: &'static str,
    pub(crate) beijing_time: &'static str,
}

/// The navigation.
#[derive(Debug)]
pub(crate) struct Nav {
    pub(crate) home: &'static str,
    pub(crate) download: &'static str,
    pub(crate) plans: &'static str,
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
    pub(crate) points: [&'static str; 3],
    pub(crate) download: &'static str,
    pub(crate) learn_more: &'static str,
    /// The button that stops and restarts the live scene.
    pub(crate) pause_motion: &'static str,
    pub(crate) intro_title: &'static str,
    pub(crate) intro_body: &'static str,
    /// What the service promises, in the order the home page draws their
    /// icons: steady, clear to see, discreet, mainland sites direct.
    pub(crate) features: [Item; 4],
    pub(crate) steps_title: &'static str,
    pub(crate) steps: [Item; 4],
    pub(crate) note: &'static str,
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
    pub(crate) how_title: &'static str,
    pub(crate) how_steps: [&'static str; 4],
    pub(crate) sections: [Item; 5],
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
