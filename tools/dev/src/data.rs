//! The checks of data plugins handed in as files, run without a file
//! system: what the browser's emulator (`shells/web`) answers when it is
//! asked to check the plugin being made. The same as `paperpaw-dev check
//! --quick` on the plugin's folder, the emulated libraries kept in memory
//! and no pictures written.

use std::path::PathBuf;

use paperpaw_emulator::profiles::Profile;

use crate::cli::{Context, run_command};
use crate::report::{Finding, Report};
use crate::target::{DataPlugin, Target};

/// The commands that look at data plugins, in the order they run.
pub const COMMANDS: &[&str] = &["package", "lint", "visual", "reach"];

/// Checks `plugins` on `profiles` with `commands` (each of [`COMMANDS`];
/// none: all of them), one variant a profile (English, light, the
/// default look); the report. A command that cannot run (the plugin is
/// refused at start) is an error in it, and the others run still.
pub fn check(
    plugins: Vec<DataPlugin>,
    profiles: Vec<&'static Profile>,
    commands: &[&str],
) -> Result<Report, String> {
    let commands = if commands.is_empty() {
        COMMANDS
    } else {
        commands
    };
    if let Some(unknown) = commands.iter().find(|c| !COMMANDS.contains(c)) {
        return Err(format!(
            "`{unknown}` is no check of data plugins (they are {})",
            COMMANDS.join(", ")
        ));
    }
    let mut target = Target::default();
    for plugin in plugins {
        target.take_data(plugin);
    }
    let mut context = Context::new(target, PathBuf::new());
    context.in_memory = true;
    context.quick = true;
    if !profiles.is_empty() {
        context.profiles = profiles;
    }
    let mut report = Report::default();
    for command in COMMANDS.iter().filter(|c| commands.contains(c)) {
        report.ran.push((*command).to_string());
        if let Err(error) = run_command(command, &context, &mut report) {
            report.add(Finding::error("dev.run", *command, error));
        }
    }
    Ok(report)
}
