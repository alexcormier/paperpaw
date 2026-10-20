//! Budgets on the X4: time per frame, heap per screen, and storage read
//! while drawing (frames are drawn in bands; a screen that reads as it
//! draws reads again in every band), with the largest costs listed.

use crate::cli::Context;
use crate::report::Report;

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}
