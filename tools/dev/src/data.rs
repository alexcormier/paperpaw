//! The checks of data plugins handed in as files, and of plugins with
//! code added to the app, run without a file system: what the browser's
//! emulator (`shells/web`) answers when it is asked to check the plugin
//! being made. The same as `paperpaw-dev check --quick` on the plugin's
//! folder (or `--plugin <id>` in its repository), the emulated libraries
//! kept in memory and no pictures written.

use std::path::PathBuf;

use paperpaw_emulator::profiles::Profile;

use crate::cli::{Context, run_command};
use crate::report::{Finding, Report};
use crate::target::Target;

/// The commands that look at data plugins, in the order they run.
pub const COMMANDS: &[&str] = &["package", "lint", "visual", "reach"];

/// The commands that look at plugins with code, in the order they run.
pub const CODE_COMMANDS: &[&str] = &["lint", "visual", "reach", "perf"];

/// Checks the plugins of `target` (its data plugins; the plugins with
/// code its `add` adds, those it names) on `profiles` with `commands`
/// (each of [`COMMANDS`], of [`CODE_COMMANDS`] too where it adds code;
/// none: all of [`COMMANDS`]), one variant a profile (English, light,
/// the default look); the report. A command that cannot run (the plugin
/// is refused at start) is an error in it, and the others run still.
pub fn check(
    target: Target,
    profiles: Vec<&'static Profile>,
    commands: &[&str],
) -> Result<Report, String> {
    let commands = if commands.is_empty() {
        COMMANDS
    } else {
        commands
    };
    let known = |c: &&str| COMMANDS.contains(c) || (target.add.is_some() && CODE_COMMANDS.contains(c));
    if let Some(unknown) = commands.iter().find(|c| !known(c)) {
        let what = if target.add.is_some() {
            CODE_COMMANDS
        } else {
            COMMANDS
        };
        return Err(format!(
            "`{unknown}` is no check of these plugins (they are {})",
            what.join(", ")
        ));
    }
    let mut context = Context::new(target, PathBuf::new());
    context.in_memory = true;
    context.quick = true;
    if !profiles.is_empty() {
        context.profiles = profiles;
    }
    let mut report = Report::default();
    let order = COMMANDS.iter().chain(CODE_COMMANDS.iter().filter(|c| !COMMANDS.contains(c)));
    for command in order.filter(|c| commands.contains(c)) {
        report.ran.push((*command).to_string());
        if let Err(error) = run_command(command, &context, &mut report) {
            report.add(Finding::error("dev.run", *command, error));
        }
    }
    Ok(report)
}
