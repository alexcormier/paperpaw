//! A data plugin installed on an emulated device, as the store installs
//! it: its folder copied into `plugins/.<id>.part`, then
//! `Command::InstallPlugin`, so the kernel's own loader decides what it
//! takes (`crates/paperpaw-core/src/data_plugin.rs` and the plugin
//! registration). What the kernel warns about is reported; then what the
//! plugin set is compared with the app it joined.
//!
//! The command comes from a small plugin of the tool's
//! ([`Installing`]), which hears an event of its own and asks for the
//! installation, as the store plugin would.

use std::sync::Mutex;

use paperpaw_core::{Kernel, KernelBuilder, WarningSource};
use paperpaw_sdk::PluginState;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::{
    Command, Id, Manifest, Params, Plugin, RegisterError, Registrar, SDK_VERSION, Version,
};

use super::Release;
use super::folder::{Folder, Line};
use crate::AddPlugins;
use crate::cli::Context;
use crate::report::{Finding, Report};
use crate::session::{self, Session};
use crate::target::{DataPlugin, Target};
use crate::variant::Variant;

/// The tool's installing plugin (its namespace starts with the probe's,
/// so the checks leave it out).
const PLUGIN: &str = "paperpaw-dev-install";
/// Asks it to install: `plugin` (the id) from `folder`.
const INSTALL: Event<Params> = Event::new("paperpaw-dev-install.install");

/// What the target adds besides the installing plugin.
static TARGET: Mutex<Option<AddPlugins>> = Mutex::new(None);

/// The installing plugin and the target's plugins.
fn add(builder: KernelBuilder) -> KernelBuilder {
    let builder = builder.plugin(Installing);
    match TARGET.lock().ok().and_then(|t| *t) {
        Some(add) => add(builder),
        None => builder,
    }
}

#[derive(Debug)]
struct Installing;

impl Plugin for Installing {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static(PLUGIN),
            "paperpaw-dev install",
            Version::new(0, 1, 0),
        )
        .description("Installs data plugins as the store does, for paperpaw-dev.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.on(INSTALL, |_, params, out| {
            if let (Some(plugin), Some(folder)) = (params.get("plugin"), params.get("folder"))
                && let Ok(id) = Id::parse(plugin)
            {
                out.push(Command::InstallPlugin(id, folder.to_string()));
            }
        })
    }
}

/// A context for sessions without the data plugins looked at (they are
/// installed while running instead), with the installing plugin.
fn context_for(context: &Context) -> Context {
    if let Ok(mut target) = TARGET.lock() {
        *target = context.target.add;
    }
    let mut own = Context::new(
        Target {
            add: Some(add),
            ..Target::default()
        },
        context.out.clone(),
    );
    own.keep = context.keep;
    own.settings.clone_from(&context.settings);
    own
}

/// The variants a plugin is installed in: one a profile.
fn variants(context: &Context) -> Vec<Variant> {
    context.variants_per_profile()
}

/// Installs `plugin` on a device of every profile and reports what the
/// kernel says and what does not fit the app.
pub fn check(
    context: &Context,
    plugin: &DataPlugin,
    folder: Option<&Folder>,
    report: &mut Report,
) -> Result<(), String> {
    let Some(folder) = folder else {
        return Ok(());
    };
    let id = folder.id(plugin).to_string();
    if Id::parse(&id).is_err() || !paperpaw_sdk::install::is_plugin_name(&id) {
        report.add(
            Finding::info(
                "package.device",
                "plugin.ini",
                "Not installed on a device: its id is unusable (see above).",
            )
            .plugin(plugin.id.clone()),
        );
        return Ok(());
    }
    let own = context_for(context);
    let start = session::Start {
        settings: context.settings.clone(),
        ..session::Start::default()
    };
    for variant in variants(context) {
        let mut session = Session::start(&own, &variant, &start)?;
        let findings = install(&mut session, plugin, folder, &id)?;
        for finding in findings {
            report.add(finding.plugin(plugin.id.clone()).variant(variant.name()));
        }
    }
    Ok(())
}

/// Installs the plugin on the session's device; what was found.
fn install(
    session: &mut Session,
    plugin: &DataPlugin,
    folder: &Folder,
    id: &str,
) -> Result<Vec<Finding>, String> {
    let mut out = Vec::new();
    let staging = format!("plugins/.{id}.part");
    session::copy_dir(&plugin.folder, &session.root().join(&staging))?;
    let event = INSTALL.id().clone();
    let params = Params::new()
        .with("plugin", id.to_string())
        .with("folder", staging.clone());
    // The device reports the kernel's warnings after every call (on the
    // console), so they are taken within it.
    let mut warnings = Vec::new();
    session.device.call(|kernel, now| {
        drop(kernel.take_warnings());
        let effects = kernel.send_event(now, event, params);
        warnings = kernel.take_warnings();
        effects
    });
    session.settle();
    let kernel = &mut session.device.kernel;
    let installed = format!("plugins/{id}/");
    for warning in warnings {
        let about_it = match &warning.source {
            WarningSource::Plugin(source) => source.as_str() == id,
            WarningSource::Runtime => {
                warning.message.contains(&staging) || warning.message.contains(&installed)
            }
            _ => false,
        };
        // Defaults for settings are checked below, with their lines.
        if !about_it || warning.message.starts_with("default for `") {
            continue;
        }
        let message = warning
            .message
            .replace(&format!("{staging}/"), "")
            .replace(&installed, "");
        let (place, message) = match message.split_once(": ") {
            Some((file, rest)) if folder.file(file).is_some() => {
                (file.to_string(), rest.to_string())
            }
            _ => (String::from("plugin.ini"), message.clone()),
        };
        let place = match warning.line {
            Some(line) => format!("{place}:{line}"),
            None => place,
        };
        let finding = Finding::error(
            "package.device",
            place,
            format!("The device says: {message}."),
        );
        // A font is loaded more than once (for reading, for the status).
        if !out.contains(&finding) {
            out.push(finding);
        }
    }
    let state = kernel
        .plugins()
        .iter()
        .find(|p| p.manifest.id.as_str() == id)
        .map(|p| p.state);
    match state {
        Some(PluginState::Running) => {}
        Some(PluginState::Off) => out.push(Finding::warning(
            "package.device",
            "plugin.ini",
            "Installed, but switched off.",
        )),
        _ => {
            out.push(Finding::error(
                "package.device",
                "plugin.ini",
                "The device did not load the plugin (see the other findings).",
            ));
            return Ok(out);
        }
    }
    settings(kernel, &folder.settings, &mut out);
    presets(kernel, id, folder, &mut out);
    keymap(kernel, &folder.keymap, &mut out);
    extends(kernel, &folder.manifest.extends, &mut out);
    Ok(out)
}

/// `settings.ini`: every key names a setting of the app, every value one
/// it takes.
fn settings(kernel: &Kernel, lines: &[Line], out: &mut Vec<Finding>) {
    for line in lines {
        let place = format!("settings.ini:{}", line.line);
        if line.section.is_empty() {
            out.push(Finding::error(
                "package.settings",
                place,
                format!("`{}` is outside a section; settings go under the first part of their id, e.g. [reader] font-size = 14.", line.key),
            ));
            continue;
        }
        let id = format!("{}.{}", line.section, line.key);
        setting(
            kernel,
            &id,
            &line.value,
            &place,
            "package.settings",
            Level::Error,
            out,
        );
    }
}

#[derive(Clone, Copy)]
enum Level {
    Error,
    Warning,
}

/// One setting and its value, as `settings.ini` and presets give them.
fn setting(
    kernel: &Kernel,
    id: &str,
    value: &str,
    place: &str,
    check: &'static str,
    unknown: Level,
    out: &mut Vec<Finding>,
) {
    let Some(spec) = kernel.settings().spec(id) else {
        let message = format!(
            "`{id}` is no setting of the app; check its id in Settings (the help names it) or in the plugin's documentation."
        );
        out.push(match unknown {
            Level::Error => Finding::error(check, place, message),
            Level::Warning => Finding::warning(check, place, message),
        });
        return;
    };
    if spec.kind.parse(value.trim()).is_none() {
        out.push(Finding::error(
            check,
            place,
            format!(
                "`{}` is no value of `{id}`, so PaperPaw passes over it.",
                value.trim()
            ),
        ));
    }
}

/// `presets.ini`: a kind some screen offers, a name and a sentence that
/// are shown, settings that exist with values they take.
fn presets(kernel: &Kernel, id: &str, folder: &Folder, out: &mut Vec<Finding>) {
    // The kinds other plugins' presets have: those some screen offers.
    let kinds: Vec<String> = kernel
        .settings()
        .presets()
        .iter()
        .filter(|p| p.id.namespace() != id)
        .map(|p| p.kind.as_str().to_string())
        .collect();
    let mut sections: Vec<(&str, u32)> = Vec::new();
    for line in &folder.presets {
        if !line.section.is_empty() && !sections.iter().any(|(s, _)| *s == line.section) {
            sections.push((&line.section, line.line));
        }
    }
    for (section, first) in sections {
        let lines: Vec<&Line> = folder
            .presets
            .iter()
            .filter(|l| l.section == section)
            .collect();
        let value = |key: &str| lines.iter().find(|l| l.key == key).map(|l| l.value.trim());
        let place = format!("presets.ini:{first}");
        match value("kind") {
            None => out.push(Finding::warning(
                "package.presets",
                place.clone(),
                format!("[{section}] has no `kind`, so no screen offers it; home screen layouts are `kind = home.layout`."),
            )),
            Some(kind) if !kinds.iter().any(|k| k == kind) => out.push(Finding::warning(
                "package.presets",
                place.clone(),
                format!("No plugin of the app offers presets of the kind `{kind}`; home screen layouts are `home.layout`."),
            )),
            Some(_) => {}
        }
        for key in ["name", "description"] {
            match value(key) {
                None | Some("") => out.push(Finding::warning(
                    "package.presets",
                    place.clone(),
                    format!("[{section}] has no `{key}`, so it is shown without one."),
                )),
                Some(text)
                    if looks_like_key(text, id) && kernel.translator().get(text).is_none() =>
                {
                    out.push(Finding::warning(
                        "package.presets",
                        place.clone(),
                        format!("`{text}` looks like a translation key, but no catalog has it, so it is shown as it is; add it to i18n/en.ini."),
                    ));
                }
                Some(_) => {}
            }
        }
        for line in &lines {
            let at = format!("presets.ini:{}", line.line);
            match line.key.as_str() {
                "kind" | "name" | "description" => {}
                key if key.contains('.') => setting(
                    kernel,
                    key,
                    &line.value,
                    &at,
                    "package.presets",
                    Level::Warning,
                    out,
                ),
                key => out.push(Finding::warning(
                    "package.presets",
                    at,
                    format!("`{key}` is not read: a preset has kind, name and description, then settings by their whole id (`home.rows = 6`)."),
                )),
            }
        }
    }
}

/// Whether `text` is meant as a translation key of plugin `id`.
fn looks_like_key(text: &str, id: &str) -> bool {
    !text.contains(' ')
        && text
            .strip_prefix(id)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// `keymap.ini`: actions that exist.
fn keymap(kernel: &Kernel, lines: &[Line], out: &mut Vec<Finding>) {
    for line in lines {
        let action = line.value.trim();
        if action == "none" || line.key == "gestures.policy" {
            continue;
        }
        if kernel.actions().get(action).is_none() {
            out.push(Finding::warning(
                "package.keymap",
                format!("keymap.ini:{}", line.line),
                format!("`{action}` is no action of the app, so the key does nothing."),
            ));
        }
    }
}

/// `[extends]`: fonts that exist.
fn extends(kernel: &Kernel, lines: &[Line], out: &mut Vec<Finding>) {
    for line in lines {
        let font = line.value.trim();
        if Id::parse(font).is_ok() && !kernel.fonts().contains(font) {
            out.push(Finding::error(
                "package.fonts",
                format!("plugin.ini:{}", line.line),
                format!("`{font}` is no font of the app, so the pack extends nothing; e.g. fonts-default.literata."),
            ));
        }
    }
}

/// Plugins with code named with `--plugin`: the SDK each was built for,
/// against this PaperPaw and the release.
pub fn code_plugins(
    context: &Context,
    ids: &[&String],
    release: Option<&Release>,
    report: &mut Report,
) -> Result<(), String> {
    let own = Context::new(
        Target {
            add: context.target.add,
            ..Target::default()
        },
        context.out.clone(),
    );
    let session = Session::new(&own, &Variant::x4())?;
    for id in ids {
        let info = session
            .device
            .kernel
            .plugins()
            .iter()
            .find(|p| p.manifest.id.as_str() == id.as_str())
            .cloned();
        let Some(info) = info else {
            report.add(
                Finding::error(
                    "package.release",
                    id.as_str(),
                    "No such plugin in the app: add it in the harness (paperpaw_dev::main), or name it by its id.",
                )
                .plugin(id.as_str()),
            );
            continue;
        };
        let sdk = info.manifest.sdk;
        report.add(
            Finding::info(
                "package.release",
                "manifest",
                format!(
                    "Built for SDK {sdk}, version {}; this PaperPaw is SDK {SDK_VERSION}.",
                    info.manifest.version
                ),
            )
            .plugin(id.as_str()),
        );
        if let Some(release) = release {
            let line = format!("{}.{}", release.version.major, release.version.minor);
            if release.loads_code(&sdk) {
                report.add(
                    Finding::info(
                        "package.release",
                        "manifest",
                        format!("PaperPaw {line} loads it (SDK {sdk})."),
                    )
                    .plugin(id.as_str()),
                );
            } else {
                report.add(
                    Finding::error(
                        "package.release",
                        "manifest",
                        format!(
                            "Built for SDK {sdk}, which PaperPaw {line} does not load (it loads plugins for SDK {line}.x up to {}); build it against paperpaw-sdk {line}.",
                            release.version
                        ),
                    )
                    .plugin(id.as_str()),
                );
            }
        }
    }
    Ok(())
}
