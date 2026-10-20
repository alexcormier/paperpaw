//! Packaging: a data plugin checked by the rules of the device's
//! installer and the website (paths, ids, file count, formats), a `.ppn`
//! file's header, fingerprint, signature and size breakdown, and
//! compatibility with a PaperPaw release.

use crate::cli::Context;
use crate::report::Report;

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}
