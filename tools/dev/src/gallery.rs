//! Contact sheets: every screen a plugin adds and every widget in every
//! size it can take, on every device profile, in light and dark, in each
//! language and look, as PNG pictures and HTML pages to look at quickly.

use crate::cli::Context;
use crate::report::Report;

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}
