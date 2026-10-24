//! Sending the highlights of the notes files that wait, one file after
//! the other, a request each.
//!
//! An export puts its notes file in `waiting.txt` (a path per line, see
//! `docs/data-files.md`) before anything is sent; it leaves only once
//! Readwise took its highlights, or when it has none (or was deleted).
//! What fails stays waiting and goes with the next sending: the next
//! export, or Wi-Fi coming back where PaperPaw manages it. A token
//! Readwise refuses is let go, so nothing is tried by itself until
//! another one is kept, or one is entered with the next export.
//!
//! Each request's body is written to a file first, a highlight at a
//! time as the notes are read, and sent from there; Readwise's answer
//! goes to a file too, unread. So only one highlight is held at a time.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::files::for_each_line;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Arg, Command, Commands, Env, HttpRequest, Params, Storage, StorageError};

use crate::{PLUGIN, Parser, SECRET, TOKEN_ENTERED};

/// Readwise's answer; parameter [`COUNT`], the highlights sent.
pub const SENT: Event<HttpReply> = Event::new("readwise.sent");
const COUNT: &str = "count";
const URL: &str = "https://readwise.io/api/v2/highlights/";
/// The files of the plugin, in its folder.
const WAITING: &str = "waiting.txt";
const BODY: &str = "body.json";
const ANSWER: &str = "answer.json";

/// A file in the plugin's folder.
fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// The notes files waiting to be sent, the oldest first.
pub fn waiting(storage: &dyn Storage) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let _ = for_each_line(storage, &file(WAITING), &mut |line| {
        if !line.is_empty() && !paths.iter().any(|p| p == line) {
            paths.push(line.to_string());
        }
        ControlFlow::Continue(())
    });
    paths
}

/// Adds the notes file `path` to those waiting, at the end.
pub fn add_waiting(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    rewrite(storage, path, Some(path))
}

/// The notes file `path` waits no more.
fn remove_waiting(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    rewrite(storage, path, None)
}

/// Writes `waiting.txt` anew without the line `key`, with `line` at the
/// end if given: through a copy, line by line. On any error the list is
/// left as it was.
fn rewrite(storage: &dyn Storage, key: &str, line: Option<&str>) -> Result<(), StorageError> {
    let (path, copy) = (file(WAITING), file(&format!("{WAITING}.new")));
    storage.write(&copy, &[])?;
    let mut result = Ok(());
    let read = for_each_line(storage, &path, &mut |old| {
        if old.is_empty() || old == key {
            return ControlFlow::Continue(());
        }
        result = storage.append(&copy, format!("{old}\n").as_bytes());
        if result.is_ok() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    });
    match read {
        Ok(()) | Err(StorageError::NotFound) => {}
        Err(e) => result = Err(e),
    }
    if let Some(line) = line {
        result = result.and_then(|()| storage.append(&copy, format!("{line}\n").as_bytes()));
    }
    match result {
        Ok(()) => storage.rename(&copy, &path),
        Err(e) => {
            let _ = storage.remove(&copy);
            Err(e)
        }
    }
}

/// Writes the body of the request for the notes file `path` to the file
/// `body`, a highlight at a time: how many highlights it holds.
fn write_body(storage: &dyn Storage, path: &str, body: &str) -> Result<usize, StorageError> {
    storage.write(body, b"{\"highlights\":[")?;
    let mut parser = Parser::default();
    let mut count = 0;
    let mut written = Ok(());
    for_each_line(storage, path, &mut |line| {
        if let Some(highlight) = parser.line(line) {
            written = append(storage, body, &parser.json(&highlight), &mut count);
        }
        match written {
            Ok(()) => ControlFlow::Continue(()),
            Err(_) => ControlFlow::Break(()),
        }
    })?;
    written?;
    if let Some(highlight) = parser.finish() {
        append(storage, body, &parser.json(&highlight), &mut count)?;
    }
    storage.append(body, b"]}")?;
    Ok(count)
}

/// Appends the highlight `json` to the file `body`, the `count`th.
fn append(
    storage: &dyn Storage,
    body: &str,
    json: &str,
    count: &mut usize,
) -> Result<(), StorageError> {
    if *count > 0 {
        storage.append(body, b",")?;
    }
    storage.append(body, json.as_bytes())?;
    *count += 1;
    Ok(())
}

/// What started a sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// A book's notes were exported, or the token entered after.
    Export,
    /// By itself, when Wi-Fi came: says only what went well.
    Retry,
}

/// A sending.
#[derive(Debug, Default)]
pub struct Run {
    running: bool,
    quiet: bool,
    /// The token it sends with.
    token: String,
    queue: Vec<String>,
    /// The notes file whose highlights are on their way.
    current: Option<String>,
    /// Highlights sent, notes files not sent.
    sent: i64,
    failed: usize,
    /// The token Readwise refused: it stays kept, but is not sent by
    /// itself again; the next export asks for one. Another token kept,
    /// or this one entered anew, is sent again.
    refused: Option<String>,
}

impl Run {
    /// Sends the highlights that wait, with `token` if given (else the
    /// one kept), or says why it cannot.
    pub fn start(&mut self, env: &Env<'_>, start: Start, token: Option<&str>, out: &mut Commands) {
        if token.is_some() {
            self.refused = None;
        }
        let waiting = waiting(env.storage());
        if self.running {
            // Those exported meanwhile come along.
            for path in waiting {
                if !self.queue.contains(&path) && self.current.as_ref() != Some(&path) {
                    self.queue.push(path);
                }
            }
            return;
        }
        if waiting.is_empty() {
            return;
        }
        let kept = env
            .secrets()
            .get(SECRET)
            .filter(|t| self.refused.as_deref() != Some(*t));
        let Some(token) = token.or(kept) else {
            if start == Start::Export {
                out.push(Command::Notify(
                    env.i18n().tr("readwise.needs-token").to_string(),
                ));
                out.push(
                    paperpaw_sdk::ask::text(env.i18n().tr("readwise.token"), &TOKEN_ENTERED)
                        .password()
                        .command(),
                );
            }
            return;
        };
        *self = Run {
            running: true,
            quiet: start == Start::Retry,
            token: token.to_string(),
            queue: waiting,
            ..Run::default()
        };
        if !env.caps().network {
            return self.end(env, "readwise.no-network", out);
        }
        self.next(env, out);
    }

    /// Sends the next notes file's highlights, or says how it went.
    fn next(&mut self, env: &Env<'_>, out: &mut Commands) {
        let body = file(BODY);
        while !self.queue.is_empty() {
            let path = self.queue.remove(0);
            if let Err(StorageError::NotFound) = env.storage().size(&path) {
                // Its notes were deleted since.
                let _ = remove_waiting(env.storage(), &path);
                continue;
            }
            match write_body(env.storage(), &path, &body) {
                Ok(0) => {
                    let _ = remove_waiting(env.storage(), &path);
                }
                Ok(count) => {
                    out.push(Command::http(self.request(&body, count)));
                    self.current = Some(path);
                    return;
                }
                // Kept waiting, as if Readwise had failed.
                Err(_) => self.failed += 1,
            }
        }
        self.finish(env, out);
    }

    /// The request sending the `count` highlights in the file `body`.
    fn request(&self, body: &str, count: usize) -> HttpRequest {
        HttpRequest::new("POST", URL, SENT)
            .header(
                String::from("Authorization"),
                format!("Token {}", self.token),
            )
            .header(
                String::from("Content-Type"),
                String::from("application/json"),
            )
            .body_from(body.to_string())
            // Readwise lists every highlight in its answer: not needed here.
            .save_to(file(ANSWER))
            .with(Params::new().with(COUNT, count.to_string()))
    }

    /// Readwise answered.
    pub fn answered(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        let Some(path) = self.current.take().filter(|_| self.running) else {
            return;
        };
        let _ = env.storage().remove(&file(ANSWER));
        match reply.status {
            200..=299 => {
                let _ = remove_waiting(env.storage(), &path);
                let count = reply.context.get(COUNT).and_then(|c| c.parse().ok());
                self.sent += count.unwrap_or(0);
                self.next(env, out);
            }
            401 | 403 => {
                // Asked for anew with the next export; kept meanwhile, as
                // Readwise may refuse it only for a while.
                let token = core::mem::take(&mut self.token);
                self.end(env, "readwise.bad-token", out);
                self.refused = Some(token);
            }
            0 => self.end(env, "readwise.unreachable", out),
            _ => {
                self.failed += 1;
                self.next(env, out);
            }
        }
    }

    /// All notes files were tried: says how it went.
    fn finish(&mut self, env: &Env<'_>, out: &mut Commands) {
        if self.failed > 0 {
            return self.end(env, "readwise.failed", out);
        }
        if self.sent == 0 {
            return self.end(env, "readwise.nothing", out);
        }
        self.quiet = false;
        let count = self.sent;
        let text = env
            .i18n()
            .plural("readwise.sent", count, &[("count", Arg::Int(count))]);
        self.end_with(env, text, out);
    }

    /// Ends the sending with message `key`.
    fn end(&mut self, env: &Env<'_>, key: &str, out: &mut Commands) {
        self.end_with(env, env.i18n().tr(key).to_string(), out);
    }

    fn end_with(&mut self, env: &Env<'_>, text: String, out: &mut Commands) {
        for name in [BODY, ANSWER] {
            let _ = env.storage().remove(&file(name));
        }
        if !self.quiet {
            out.push(Command::Notify(text));
        }
        *self = Run::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn keeps_what_waits() {
        let s = MemoryStorage::new();
        assert!(waiting(&s).is_empty());
        add_waiting(&s, "Notes/a.md").unwrap();
        add_waiting(&s, "Notes/b.md").unwrap();
        add_waiting(&s, "Notes/a.md").unwrap();
        assert_eq!(waiting(&s), ["Notes/b.md", "Notes/a.md"]);
        remove_waiting(&s, "Notes/b.md").unwrap();
        remove_waiting(&s, "Notes/c.md").unwrap();
        assert_eq!(waiting(&s), ["Notes/a.md"]);
        assert!(s.get(&file("waiting.txt.new")).is_none());
    }

    #[test]
    fn writes_the_body_a_highlight_at_a_time() {
        let s = MemoryStorage::new().with(
            "a.md",
            "# A\n\nB\n\n## Highlights\n\n> one\n>\n> 1 %\n\n> two\n>\n> 2 %\n",
        );
        assert_eq!(write_body(&s, "a.md", "body.json"), Ok(2));
        let body = String::from_utf8(s.get("body.json").unwrap()).unwrap();
        assert!(body.starts_with("{\"highlights\":[{\"text\":\"one\""));
        assert!(body.contains("},{\"text\":\"two\""));
        assert!(body.ends_with("}]}"));
        assert_eq!(
            write_body(&s, "missing.md", "body.json"),
            Err(StorageError::NotFound)
        );
    }
}
