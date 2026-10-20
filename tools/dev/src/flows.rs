//! Scripted flows: steps (presses, taps, swipes) with assertions about
//! what the screen shows, written by hand or recorded in the emulator's
//! window, replayed on every profile they fit.

use crate::cli::Context;
use crate::report::Report;

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}
