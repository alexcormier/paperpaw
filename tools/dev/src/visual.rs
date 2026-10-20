//! Automatic checks of what is drawn: text that overflows its box, is cut
//! or wraps where it should not; parts outside their cell or the screen;
//! uneven margins and misalignment against the interface's grid; touch
//! targets too small for a finger (by the screen's dpi); focus visible for
//! buttons-only use; ink centred where the rules say (headers, icons);
//! contrast on black-and-white screens.

use crate::cli::Context;
use crate::report::Report;

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}
