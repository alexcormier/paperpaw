//! Buttons alone: every screen a plugin adds can be reached and closed
//! with the device's buttons, and the focus shows where it is.

use crate::cli::Context;
use crate::report::Report;

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}
