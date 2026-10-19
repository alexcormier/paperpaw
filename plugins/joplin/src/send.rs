//! Sending the notes that wait (see [`kept::waiting`]), one book after
//! the other, a request at a time:
//!
//! 1. Joplin Cloud and Joplin Server: sign in, unless signed in already.
//! 2. `info.json`, which every sync target has: there is Joplin data at
//!    the address, and it is not end-to-end encrypted (notes PaperPaw
//!    sent unencrypted would sit there in plain text).
//! 3. The notebook named in the settings: made once, then found again.
//! 4. Each book: its note, if it was sent before, fetched to a file and
//!    only its end read. Its text not edited since, it is written over,
//!    staying in the notebook it was moved to; edited in Joplin or put in
//!    the trash, a new note is made beside it, so nothing written there is
//!    lost. The new note is written to a file first, the body copied
//!    piece by piece from the notes, and sent from there.
//!
//! What fails stays waiting and goes with the next sending: the next
//! export, the action [`SEND`](crate::SEND) (by hand or by a rule), or
//! Wi-Fi coming back where PaperPaw manages it. A wrong password or
//! address stops those automatic tries until the account is changed.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_json::stream::{self, Event as Json};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Arg, Command, Env, HttpRequest, Params};

use crate::item::{self, NOTE, NOTEBOOK};
use crate::kept::{self, Sent};
use crate::markdown;
use crate::service::{self, Auth, Service};
use crate::{DONE, Finished, NOTEBOOK as NOTEBOOK_SETTING, PASSWORD, SCREEN, USER};

/// The answers, a step each.
pub const SIGNED_IN: Event<HttpReply> = Event::new("joplin.signed-in");
pub const CHECKED: Event<HttpReply> = Event::new("joplin.checked");
pub const NOTEBOOK_FOUND: Event<HttpReply> = Event::new("joplin.notebook-found");
pub const NOTEBOOK_MADE: Event<HttpReply> = Event::new("joplin.notebook-made");
pub const NOTE_FOUND: Event<HttpReply> = Event::new("joplin.note-found");
pub const NOTE_SENT: Event<HttpReply> = Event::new("joplin.note-sent");

/// The notebook's name when none is set.
pub const DEFAULT_NOTEBOOK: &str = "PaperPaw";
/// The id the notebook was made with; parameter of [`NOTEBOOK_MADE`].
const ID: &str = "id";
/// The files of a sending, in the plugin's folder.
const INFO: &str = "info.json";
const FOUND: &str = "found.md";
const NOTE_FILE: &str = "note.md";

/// What started a sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// A book's notes were exported.
    Export,
    /// The action, chosen or run by a rule.
    Action,
    /// By itself, when Wi-Fi came: says only what went well.
    Retry,
}

/// How a sending ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// To be tried again by itself.
    Again,
    /// At the account or address: not tried by itself until they change.
    Stop,
}

/// The account set up, if all of it is.
struct Account<'a> {
    service: Service,
    base: String,
    user: &'a str,
    password: &'a str,
}

impl<'a> Account<'a> {
    fn of(env: &Env<'a>) -> Option<Self> {
        let service = Service::of(env);
        let base = service.base(env);
        let user = env.settings.read(&USER).trim();
        let password = env.secrets.get(PASSWORD).unwrap_or_default();
        (!base.is_empty() && !user.is_empty() && !password.is_empty()).then_some(Account {
            service,
            base,
            user,
            password,
        })
    }
}

/// The notebook's name, as set.
pub fn notebook_name<'a>(env: &Env<'a>) -> &'a str {
    match env.settings.read(&NOTEBOOK_SETTING).trim() {
        "" => DEFAULT_NOTEBOOK,
        name => name,
    }
}

/// Milliseconds since 1970 now, if the device knows the time.
fn now_ms(env: &Env<'_>) -> Option<i64> {
    env.status.clock.map(|clock| clock.unix_millis_at(env.now))
}

/// The book being sent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Book {
    path: String,
    /// Its note, as sent before or as being sent now.
    note: Option<Sent>,
}

/// A sending, and what is kept between them.
#[derive(Debug, Default)]
pub struct Run {
    running: bool,
    quiet: bool,
    /// Joplin Cloud's or Joplin Server's session, kept while PaperPaw
    /// runs.
    session: Option<String>,
    /// Signed in anew in this sending, after the session had ended.
    renewed: bool,
    /// The last sending stopped at the account or address.
    blocked: bool,
    queue: Vec<String>,
    book: Option<Book>,
    notebook: String,
    sent: usize,
    failed: usize,
    /// Notes changed in Joplin, so made anew beside them.
    apart: usize,
}

impl Run {
    /// The account was changed: signed in anew, tried again by itself.
    pub fn account_changed(&mut self) {
        self.session = None;
        self.blocked = false;
    }

    /// Sends the notes that wait, or says why it cannot.
    pub fn start(&mut self, env: &Env<'_>, start: Start, out: &mut Vec<Command>) {
        let waiting = kept::waiting(env.storage);
        if self.running {
            // Those exported meanwhile come along.
            let current = self.book.as_ref().map(|b| b.path.as_str());
            for path in waiting {
                if !self.queue.contains(&path) && current != Some(path.as_str()) {
                    self.queue.push(path);
                }
            }
            return;
        }
        if start == Start::Retry && (self.blocked || waiting.is_empty()) {
            return;
        }
        self.quiet = start == Start::Retry;
        if waiting.is_empty() {
            return self.end(env, "joplin.nothing", &[], End::Again, out);
        }
        if !env.caps.network {
            return self.end(env, "joplin.no-network", &[], End::Again, out);
        }
        if Account::of(env).is_none() {
            if start == Start::Export {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
            }
            return self.end(env, "joplin.setup-needed", &[], End::Stop, out);
        }
        if now_ms(env).is_none() {
            return self.end(env, "joplin.no-time", &[], End::Again, out);
        }
        *self = Run {
            running: true,
            quiet: self.quiet,
            session: self.session.take(),
            queue: waiting,
            ..Run::default()
        };
        self.sign_in_or_check(env, out);
    }

    fn sign_in_or_check(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        let Some(account) = Account::of(env) else {
            return self.end(env, "joplin.setup-needed", &[], End::Stop, out);
        };
        if account.service.signs_in() && self.session.is_none() {
            let request =
                service::sign_in(&account.base, account.user, account.password, &SIGNED_IN);
            return out.push(Command::Http(request));
        }
        let mut request = self.request(env, INFO, "GET", &CHECKED);
        request.save_to = Some(kept::file(INFO));
        out.push(Command::Http(request));
    }

    /// A request for the file `name` of the sync target.
    fn request(
        &self,
        env: &Env<'_>,
        name: &str,
        method: &str,
        reply: &Event<HttpReply>,
    ) -> HttpRequest {
        let service = Service::of(env);
        let auth = match &self.session {
            Some(session) if service.signs_in() => Auth::Session(session),
            _ => Auth::Basic {
                user: env.settings.read(&USER).trim(),
                password: env.secrets.get(PASSWORD).unwrap_or_default(),
            },
        };
        service::request(service, &service.base(env), name, method, auth, reply)
    }

    /// A request putting the file `name`; its body is the caller's.
    fn put(&self, env: &Env<'_>, name: &str, reply: &Event<HttpReply>) -> HttpRequest {
        let mut request = self.request(env, name, "PUT", reply);
        request.headers.push((
            String::from("Content-Type"),
            String::from("application/octet-stream"),
        ));
        request
    }

    /// Answers that are no success, the same for every step.
    fn trouble(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        match reply.status {
            401 | 403 => self.unauthorised(env, out),
            0 => self.end(env, "joplin.unreachable", &[], End::Again, out),
            status => {
                let args = [("status", Arg::Int(i64::from(status)))];
                self.end(env, "joplin.failed", &args, End::Again, out);
            }
        }
    }

    /// Not let in: the session ended (signed in anew, once), or the user
    /// name or password is wrong.
    fn unauthorised(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        let had_session = self.session.take().is_some();
        if had_session && !self.renewed {
            self.renewed = true;
            if let Some(book) = self.book.take() {
                self.queue.insert(0, book.path);
            }
            return self.sign_in_or_check(env, out);
        }
        self.end(env, "joplin.bad-login", &[], End::Stop, out);
    }

    pub fn signed_in(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        if !self.running {
            return;
        }
        match reply.status {
            200..=299 => {
                let body = paperpaw_json::object(&reply.body);
                match paperpaw_json::text(&body, "id") {
                    Some(id) if !id.is_empty() => {
                        self.session = Some(id.to_string());
                        self.sign_in_or_check(env, out);
                    }
                    _ => self.end(env, "joplin.no-server", &[], End::Stop, out),
                }
            }
            400 | 401 | 403 => self.end(env, "joplin.bad-login", &[], End::Stop, out),
            404 | 405 => self.end(env, "joplin.no-server", &[], End::Stop, out),
            _ => self.trouble(env, reply, out),
        }
    }

    pub fn checked(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        if !self.running {
            return;
        }
        match reply.status {
            200..=299 => {
                if encrypted(env) {
                    return self.end(env, "joplin.encrypted", &[], End::Stop, out);
                }
                self.find_notebook(env, out);
            }
            404 => self.end(env, "joplin.no-data", &[], End::Stop, out),
            _ => self.trouble(env, reply, out),
        }
    }

    /// The notebook made before, or a new one.
    fn find_notebook(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        match kept::notebook(env.storage) {
            Some((name, id)) if name == notebook_name(env) && item::is_id(&id) => {
                let mut request = self.request(env, &item::file_name(&id), "GET", &NOTEBOOK_FOUND);
                request.save_to = Some(kept::file(FOUND));
                request.params = Params::new().with(ID, id);
                out.push(Command::Http(request));
            }
            _ => self.make_notebook(env, out),
        }
    }

    pub fn notebook_found(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        if !self.running {
            return;
        }
        match reply.status {
            200..=299 => {
                let props = item::read_props(env.storage, &kept::file(FOUND)).unwrap_or_default();
                let _ = env.storage.remove(&kept::file(FOUND));
                match reply.context.get(ID) {
                    Some(id) if props.kind == NOTEBOOK && !props.deleted => {
                        self.notebook = id.to_string();
                        self.next_book(env, out);
                    }
                    _ => self.make_notebook(env, out),
                }
            }
            404 => self.make_notebook(env, out),
            _ => self.trouble(env, reply, out),
        }
    }

    fn make_notebook(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        let now = now_ms(env).unwrap_or_default();
        let name = notebook_name(env);
        let old = kept::notebook(env.storage).map(|(_, id)| id);
        let id = item::new_id(&format!("paperpaw notebook {name} {now}"), old.as_deref());
        let mut request = self.put(env, &item::file_name(&id), &NOTEBOOK_MADE);
        request.body = Some(item::notebook(&id, name, &item::time(now)));
        request.params = Params::new().with(ID, id);
        out.push(Command::Http(request));
    }

    pub fn notebook_made(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        if !self.running {
            return;
        }
        let Some(id) = reply.context.get(ID).filter(|_| reply.ok()) else {
            return self.trouble(env, reply, out);
        };
        let _ = kept::keep_notebook(env.storage, notebook_name(env), id);
        self.notebook = id.to_string();
        self.next_book(env, out);
    }

    /// The next book: its note fetched if it was sent before, else made.
    fn next_book(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        while !self.queue.is_empty() {
            let path = self.queue.remove(0);
            if !env.storage.exists(&path) {
                // Its notes were deleted since.
                let _ = kept::remove_waiting(env.storage, &path);
                continue;
            }
            let note = kept::sent(env.storage, &path);
            self.book = Some(Book {
                path,
                note: note.clone(),
            });
            let Some(note) = note else {
                return self.write_note(env, None, out);
            };
            let mut request = self.request(env, &item::file_name(&note.id), "GET", &NOTE_FOUND);
            request.save_to = Some(kept::file(FOUND));
            return out.push(Command::Http(request));
        }
        self.finish(env, out);
    }

    pub fn note_found(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        let Some(book) = self.book.as_ref().filter(|_| self.running) else {
            return;
        };
        match reply.status {
            200..=299 => {
                let found = kept::file(FOUND);
                let props = item::read_props(env.storage, &found).unwrap_or_default();
                // Edited in Joplin since, if its text is not what PaperPaw
                // wrote (its times tell less: moving it to another
                // notebook changes them too, some edits do not).
                let text = item::text_hash(env.storage, &found, props.text_end).ok();
                let _ = env.storage.remove(&found);
                let ours = book.note.as_ref().map(|n| &n.text) == text.as_ref();
                if props.kind == NOTE && ours && !props.deleted {
                    self.write_note(env, Some(props), out);
                } else {
                    if !props.deleted {
                        self.apart += 1;
                    }
                    self.write_note(env, None, out);
                }
            }
            // Deleted in Joplin: made anew.
            404 => self.write_note(env, None, out),
            401 | 403 | 0 => self.trouble(env, reply, out),
            _ => self.book_failed(env, out),
        }
    }

    /// Writes the book's note and sends it: over `found` (its note as
    /// Joplin has it), or as a new note.
    fn write_note(&mut self, env: &Env<'_>, found: Option<item::Props>, out: &mut Vec<Command>) {
        let Some(mut book) = self.book.take() else {
            return;
        };
        let now = now_ms(env).unwrap_or_default();
        // Later than the note found, or Joplin keeps that one (also if
        // this device's clock is behind).
        let updated = item::time(found.as_ref().map_or(now, |p| now.max(p.updated + 1)));
        let old = book.note.as_ref().map(|n| n.id.as_str());
        let id = match (&found, old) {
            (Some(_), Some(old)) => old.to_string(),
            _ => item::new_id(&format!("paperpaw note {} {now}", book.path), old),
        };
        let head = markdown::head(env.storage, &book.path).unwrap_or_default();
        let title = match head.title.as_str() {
            "" => paperpaw_sdk::files::name(&book.path).trim_end_matches(".md"),
            title => title,
        };
        // Written over, it keeps where it is and when it was made.
        let keep = |field: fn(&item::Props) -> &str, new: &'_ str| -> String {
            match found.as_ref().map(field) {
                Some(value) if !value.is_empty() => value.to_string(),
                _ => new.to_string(),
            }
        };
        let parent = keep(|p| &p.parent, &self.notebook);
        let created = keep(|p| &p.created, &updated);
        let user_created = keep(|p| &p.user_created, &created);
        let note = item::Note {
            id: &id,
            parent: &parent,
            title,
            author: &head.author,
            created: &created,
            user_created: &user_created,
            updated: &updated,
            order: found.as_ref().map_or(now, |p| p.order),
        };
        let file = kept::file(NOTE_FILE);
        let Ok(text) = note.write(env.storage, &book.path, &file) else {
            return self.book_failed(env, out);
        };
        let mut request = self.put(env, &item::file_name(&id), &NOTE_SENT);
        request.body_from = Some(file);
        out.push(Command::Http(request));
        book.note = Some(Sent { id, text });
        self.book = Some(book);
    }

    pub fn note_sent(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        let Some(book) = self.book.take().filter(|_| self.running) else {
            return;
        };
        if !reply.ok() {
            self.book = Some(book);
            return match reply.status {
                401 | 403 | 0 => self.trouble(env, reply, out),
                _ => self.book_failed(env, out),
            };
        }
        let _ = env.storage.remove(&kept::file(NOTE_FILE));
        if let Some(note) = &book.note {
            let _ = kept::keep_sent(env.storage, &book.path, note);
        }
        let _ = kept::remove_waiting(env.storage, &book.path);
        self.sent += 1;
        self.next_book(env, out);
    }

    /// This book could not be sent: it waits, the others go on.
    fn book_failed(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        self.book = None;
        self.failed += 1;
        self.next_book(env, out);
    }

    /// All books were tried: says how it went.
    fn finish(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        let (sent, failed) = (self.sent as i64, self.failed as i64);
        if self.apart > 0 && !self.quiet {
            let count = self.apart as i64;
            let text = env
                .i18n
                .plural("joplin.apart", count, &[("count", Arg::Int(count))]);
            out.push(Command::Notify(text));
        }
        if failed > 0 {
            let args = [("sent", Arg::Int(sent)), ("failed", Arg::Int(failed))];
            return self.end(env, "joplin.some-failed", &args, End::Again, out);
        }
        if sent == 0 {
            // The notes that waited were deleted.
            return self.end(env, "joplin.nothing", &[], End::Again, out);
        }
        self.quiet = false;
        let text = env
            .i18n
            .plural("joplin.sent", sent, &[("count", Arg::Int(sent))]);
        self.end_with(env, text, End::Again, out);
    }

    /// Ends the sending with message `key`.
    fn end(
        &mut self,
        env: &Env<'_>,
        key: &str,
        args: &[(&str, Arg<'_>)],
        end: End,
        out: &mut Vec<Command>,
    ) {
        self.end_with(env, env.i18n.format(key, args), end, out);
    }

    fn end_with(&mut self, env: &Env<'_>, text: String, end: End, out: &mut Vec<Command>) {
        self.running = false;
        self.blocked = end == End::Stop;
        self.queue.clear();
        self.book = None;
        for name in [INFO, FOUND, NOTE_FILE] {
            let _ = env.storage.remove(&kept::file(name));
        }
        if !self.quiet {
            out.push(Command::Notify(text.clone()));
        }
        out.push(DONE.emit(Finished { text }));
    }
}

/// Whether the sync target's `info.json`, just fetched, says its notes
/// are end-to-end encrypted.
fn encrypted(env: &Env<'_>) -> bool {
    let mut on = false;
    let path = ["e2ee".to_string(), "value".to_string()];
    stream::read_file(env.storage, &kept::file(INFO), &mut |event| {
        if let Json::Value(at, value) = event
            && at == path
        {
            on = value == "true";
        }
    });
    on
}
