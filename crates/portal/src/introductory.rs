//! Workbench actions that still need the operator.

use crate::CommandError;
use portal_store::Store;
use std::io::Write;

/// The actions the operator must take in Stripe for introductory orders.
pub(crate) fn list(store: &Store, out: &mut dyn Write) -> Result<(), CommandError> {
    let starts = store.introductory_starts()?;
    let requests = store.introductory_requests()?;
    writeln!(out, "Introductory schedules to create ({})", starts.len())?;
    for start in starts {
        let end_behavior = if start.stop { "cancel" } else { "release" };
        writeln!(
            out,
            "  order {}  customer {}  paid intent {}  start_date={}  end_date={}  end_behavior={end_behavior}",
            start.order,
            start.customer,
            start.payment,
            start.start.as_secs(),
            start.end.as_secs()
        )?;
    }
    writeln!(
        out,
        "Introductory non-renewal requests awaiting Stripe confirmation ({})",
        requests.len()
    )?;
    for request in requests {
        let schedule = request
            .schedule
            .as_ref()
            .map_or("not created yet", |id| id.as_str());
        writeln!(
            out,
            "  order {}  customer {}  schedule {schedule}  cancel at {}",
            request.order,
            request.customer,
            request.end.as_secs()
        )?;
    }
    Ok(())
}
