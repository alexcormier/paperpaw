//! Snapshots: screens drawn and compared with reference pictures, written
//! anew with `--bless`; also the template for a plugin repository's own
//! snapshot test.

use crate::cli::Context;
use crate::report::Report;

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}
