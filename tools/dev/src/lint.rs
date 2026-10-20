//! Texts: translations missing or left over, a pseudo-language that finds
//! text too long for its place, settings without help texts, ids outside
//! the plugin's namespace, and dashes where the project forbids them.

use crate::cli::Context;
use crate::report::Report;

/// The pseudo-language's code (see [`pseudo_plugin`]).
pub const PSEUDO: &str = "pseudo";

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let _ = (context, report);
    Ok(())
}

/// The files of a data plugin that adds the pseudo-language, as `(path,
/// bytes)` in the library: put in by sessions whose language is
/// [`PSEUDO`].
pub fn pseudo_plugin(context: &Context) -> Result<Vec<(String, Vec<u8>)>, String> {
    let _ = context;
    Ok(Vec::new())
}
