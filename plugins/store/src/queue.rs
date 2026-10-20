//! Installs and updates one after another: *Update all* and the
//! first-run setup's offer (`setup`) keep the plugins in a queue on the
//! storage. Each one's page at the library (with its files) is fetched
//! when its turn comes, and its download starts once the one before is
//! done; screens may come and go meanwhile. The setup's queue is quiet:
//! what became of each is kept, and said once when the queue is done.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::params;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Arg, Command, Env, HttpRequest, Params, install};

use crate::catalog::{self, Device};
use crate::{CATALOG, INSTALLER, PLUGIN, failed, trust};

/// The answer with the page of the plugin whose turn it is.
pub(crate) const FETCHED: Event<HttpReply> = Event::new("store.queued");

fn path() -> String {
    format!("{}/queue.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The plugins waiting, in order.
fn waiting(env: &Env<'_>) -> Vec<String> {
    paperpaw_sdk::read_file(env.storage, &path(), 16 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.lines().map(ToString::to_string).collect())
        .unwrap_or_default()
}

/// Where a quiet queue keeps what became of each plugin: `+<name>`
/// installed, `-<name>` not.
fn quiet_path() -> String {
    format!("{}/queue-quiet.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// Whether the queue is quiet: each plugin installed is not said on its
/// own.
pub(crate) fn quiet(env: &Env<'_>) -> bool {
    env.storage.exists(&quiet_path())
}

/// Keeps, for a quiet queue, that plugin `name` was installed (`done`) or
/// not.
pub(crate) fn note(env: &Env<'_>, name: &str, done: bool) {
    let line = format!(
        "{}{}\n",
        if done { '+' } else { '-' },
        name.replace('\n', " ")
    );
    let _ = env.storage.append(&quiet_path(), line.as_bytes());
}

/// Plugin `id`'s name where the page with its name did not come: the
/// setup's own for the plugins it offers, else its id.
fn name_of(env: &Env<'_>, id: &str) -> String {
    env.i18n
        .get(&format!("store.offer.{id}"))
        .map_or_else(|| id.to_string(), ToString::to_string)
}

/// Says, once a quiet queue is done, what it installed and what not.
fn said(env: &Env<'_>, out: &mut Vec<Command>) {
    let Some(text) = paperpaw_sdk::read_file(env.storage, &quiet_path(), 16 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return;
    };
    let _ = env.storage.remove(&quiet_path());
    let names = |mark: char| {
        text.lines()
            .filter_map(|line| line.strip_prefix(mark))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (done, not) = (names('+'), names('-'));
    if !done.is_empty() {
        out.push(Command::Notify(
            env.i18n
                .format("store.offer.done", &[("names", Arg::Str(&done))]),
        ));
    }
    if !not.is_empty() {
        out.push(Command::Notify(
            env.i18n
                .format("store.offer.not-done", &[("names", Arg::Str(&not))]),
        ));
    }
}

fn keep(env: &Env<'_>, ids: &[String]) {
    if ids.is_empty() {
        let _ = env.storage.remove(&path());
        return;
    }
    let mut text = ids.join("\n");
    text.push('\n');
    let _ = env.storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = env.storage.write(&path(), text.as_bytes());
}

/// Puts `ids` in the queue (behind those waiting) and starts the first
/// if nothing is being downloaded.
pub(crate) fn add(env: &Env<'_>, ids: &[String], out: &mut Vec<Command>) {
    let mut all = waiting(env);
    for id in ids {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }
    keep(env, &all);
    next(env, out);
}

/// Puts `ids` in a quiet queue (see [`quiet`]): the first-run setup's.
pub(crate) fn add_quietly(env: &Env<'_>, ids: &[String], out: &mut Vec<Command>) {
    let _ = env.storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    if !quiet(env) {
        let _ = env.storage.write(&quiet_path(), b"");
    }
    add(env, ids, out);
}

/// Forgets the queue: at the start, nothing is downloaded unasked.
pub(crate) fn clear(env: &Env<'_>) {
    keep(env, &[]);
    let _ = env.storage.remove(&quiet_path());
}

/// The address of plugin `id`'s page at the library.
fn page_url(env: &Env<'_>, device: &Device<'_>, id: &str) -> String {
    let list = env.settings.read(&CATALOG);
    catalog::plugin_url(list.trim(), device.name, env.i18n.language(), id)
}

/// Asks for the page of the next plugin in the queue, if nothing is being
/// downloaded.
pub(crate) fn next(env: &Env<'_>, out: &mut Vec<Command>) {
    if INSTALLER.progress(env.storage).is_some() {
        return;
    }
    let Some(device) = Device::of(env).filter(|_| env.caps.network) else {
        for id in waiting(env) {
            note(env, &name_of(env, &id), false);
        }
        keep(env, &[]);
        return said(env, out);
    };
    let mut all = waiting(env);
    while !all.is_empty() {
        let id = all.remove(0);
        if !install::is_plugin_name(&id) {
            continue;
        }
        keep(env, &all);
        let url = page_url(env, &device, &id);
        let mut request = HttpRequest::new("GET", &url, FETCHED);
        request.save_to = Some(catalog::plugin_part(&id));
        request.params = Params::new().with(params::ID, id);
        out.push(Command::Http(request));
        return;
    }
    keep(env, &[]);
    said(env, out);
}

/// The page of the plugin whose turn it is came: its download starts, or
/// the next one's turn comes.
pub(crate) fn fetched(env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
    let Some(id) = reply.context.get(params::ID).map(ToString::to_string) else {
        return;
    };
    let fetched = catalog::plugin_part(&id);
    let plugin = Device::of(env)
        .filter(|_| (200..300).contains(&reply.status))
        .and_then(|device| {
            let address = reply
                .final_url
                .clone()
                .unwrap_or_else(|| page_url(env, &device, &id));
            catalog::plugin(env.storage, &fetched, &address, &device)
        })
        // Not installed yet, or newer than the one installed.
        .filter(|plugin| {
            plugin.entry.id == id
                && (plugin.entry.installed(env).is_none() || plugin.entry.newer(env))
        });
    let _ = env.storage.remove(&fetched);
    match plugin {
        Some(plugin) => {
            trust::expect(env.storage, &plugin.entry.id, plugin.author_key.as_ref());
            if !INSTALLER.start(env, &plugin.package, out) {
                if quiet(env) {
                    note(env, &plugin.entry.name, false);
                } else {
                    out.push(failed(env, &plugin.entry.name));
                }
                next(env, out);
            }
        }
        None => {
            if quiet(env) {
                note(env, &name_of(env, &id), false);
            }
            next(env, out);
        }
    }
}
