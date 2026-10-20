//! Updates one after another: *Update all* keeps the plugins to update in
//! a queue on the storage. Each one's page at the library (with its files)
//! is fetched when its turn comes, and its download starts once the one
//! before is done, stopped, or set aside; screens may come and go
//! meanwhile. Without a connection, or after a restart, the queue waits
//! and goes on when the store is opened again (nothing is downloaded
//! unasked at the start).
//!
//! The queue is a line per plugin; the one whose turn it is stays first,
//! marked, until the next one's turn comes, so that one cut off by a
//! restart is begun anew.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::params;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Command, Env, HttpRequest, Params, install};

use crate::catalog::{self, Device};
use crate::{CATALOG, INSTALLER, PLUGIN, failed, trust};

/// The answer with the page of the plugin whose turn it is.
pub(crate) const FETCHED: Event<HttpReply> = Event::new("store.queued");

/// The mark of the plugin whose turn it is.
const TURN: &str = "> ";

fn path() -> String {
    format!("{}/queue.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The queue's lines, the one whose turn it is first and marked.
fn lines(env: &Env<'_>) -> Vec<String> {
    paperpaw_sdk::read_file(env.storage, &path(), 16 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.lines().map(ToString::to_string).collect())
        .unwrap_or_default()
}

fn keep(env: &Env<'_>, lines: &[String]) {
    if lines.is_empty() {
        let _ = env.storage.remove(&path());
        return;
    }
    let mut text = lines.join("\n");
    text.push('\n');
    let _ = env.storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = env.storage.write(&path(), text.as_bytes());
}

/// The plugins in the queue, in order (the one whose turn it is first).
pub(crate) fn waiting(env: &Env<'_>) -> Vec<String> {
    lines(env)
        .into_iter()
        .map(|line| line.strip_prefix(TURN).unwrap_or(&line).to_string())
        .collect()
}

/// Puts `ids` in the queue (behind those waiting) and starts the first
/// if nothing is being downloaded.
pub(crate) fn add(env: &Env<'_>, ids: &[String], out: &mut Vec<Command>) {
    let mut all = lines(env);
    let known = waiting(env);
    for id in ids {
        if !known.contains(id) {
            all.push(id.clone());
        }
    }
    keep(env, &all);
    next(env, out);
}

/// A download cut off by a restart: its turn again, first, when the
/// queue goes on.
pub(crate) fn interrupted(env: &Env<'_>) {
    let mut all = lines(env);
    if let Some(first) = all.first_mut()
        && let Some(id) = first.strip_prefix(TURN)
    {
        *first = id.to_string();
        keep(env, &all);
    }
}

/// The address of plugin `id`'s page at the library.
fn page_url(env: &Env<'_>, device: &Device<'_>, id: &str) -> String {
    let list = env.settings.read(&CATALOG);
    catalog::plugin_url(list.trim(), device.name, env.i18n.language(), id)
}

/// Gives the next plugin its turn (asks for its page), if nothing is
/// being downloaded: the one before is done, stopped or set aside.
pub(crate) fn next(env: &Env<'_>, out: &mut Vec<Command>) {
    if INSTALLER.progress(env.storage).is_some() {
        return;
    }
    // No connection: it waits for the next time the store is opened.
    let Some(device) = Device::of(env).filter(|_| env.caps.network) else {
        return;
    };
    let mut all = lines(env);
    if all.first().is_some_and(|first| first.starts_with(TURN)) {
        all.remove(0);
    }
    all.retain(|id| install::is_plugin_name(id));
    let Some(id) = all.first().cloned() else {
        return keep(env, &[]);
    };
    all[0] = format!("{TURN}{id}");
    keep(env, &all);
    let url = page_url(env, &device, &id);
    let mut request = HttpRequest::new("GET", &url, FETCHED);
    request.save_to = Some(catalog::plugin_part(&id));
    request.params = Params::new().with(params::ID, id);
    out.push(Command::Http(request));
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
        .filter(|plugin| plugin.entry.id == id && plugin.entry.newer(env));
    let _ = env.storage.remove(&fetched);
    match plugin {
        Some(plugin) => {
            trust::expect(env.storage, &plugin.entry.id, plugin.author_key.as_ref());
            if !INSTALLER.start(env, &plugin.package, out) {
                out.push(failed(env, &plugin.entry.name));
                next(env, out);
            }
        }
        None => next(env, out),
    }
}
