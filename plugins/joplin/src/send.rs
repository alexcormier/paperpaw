//! Sending the notes that wait (see [`kept::waiting`]), one book after
//! the other, a request at a time:
//!
//! 1. Joplin Cloud and Joplin Server: sign in, unless signed in already.
//! 2. `info.json`, which every sync target has: there is Joplin data at
//!    the address, in the version of the format PaperPaw writes
//!    ([`item::SYNC_VERSION`]), and not end-to-end encrypted (notes
//!    PaperPaw sent unencrypted would sit there in plain text).
//! 3. Its locks: none exclusive, held by a client upgrading the target
//!    (see [`locks`]).
//! 4. The notebook named in the settings: made once, then found again.
//! 5. Each book: its note, if it was sent before, fetched to a file and
//!    only its end read. Its text not edited since, it is written over,
//!    staying in the notebook it was moved to; edited in Joplin or put in
//!    the trash, a new note is made beside it, so nothing written there is
//!    lost. The new note is written to a file first, the body copied
//!    piece by piece from the notes, and sent from there. It is kept as
//!    being sent before it goes: should the answer be lost, Joplin may
//!    have it all the same, so the next sending looks for it and writes
//!    it over rather than making it again.
//!
//! What fails stays waiting and goes with the next sending: the next
//! export, the action [`SEND`](crate::SEND) (by hand or by a rule), or
//! Wi-Fi coming back where PaperPaw manages it. A wrong password or
//! address stops those automatic tries until the account is changed.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::json::stream::{self, Event as Json};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::time::UnixTime;
use paperpaw_sdk::{Arg, Command, Commands, Env, HttpRequest, Params, StorageError};

use crate::item::{self, NOTE, NOTEBOOK};
use crate::kept::{self, Sent};
use crate::locks;
use crate::markdown;
use crate::service::{self, Auth, Service};
use crate::{DONE, Finished, NOTEBOOK as NOTEBOOK_SETTING, PASSWORD, SCREEN, USER};

/// The answers, a step each.
pub const SIGNED_IN: Event<HttpReply> = Event::new("joplin.signed-in");
pub const CHECKED: Event<HttpReply> = Event::new("joplin.checked");
pub const LOCKS: Event<HttpReply> = Event::new("joplin.locks");
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
const LOCKS_FILE: &str = "locks.txt";
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
        let user = env.settings().read(&USER).trim();
        let password = env.secrets().get(PASSWORD).unwrap_or_default();
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
    match env.settings().read(&NOTEBOOK_SETTING).trim() {
        "" => DEFAULT_NOTEBOOK,
        name => name,
    }
}

/// Milliseconds since 1970 now, if the device knows the time.
fn now_ms(env: &Env<'_>) -> Option<i64> {
    env.status()
        .clock
        .map(|clock| clock.unix_millis_at(env.now()))
}

/// The book being sent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Book {
    path: String,
    /// Its note, as sent before.
    note: Option<Sent>,
    /// Its note as sent last or now, while no answer came: Joplin may
    /// have it, or still [`note`](Self::note).
    sending: Option<Sent>,
}

impl Book {
    /// The note looked for in Joplin: the one sent last.
    fn last(&self) -> Option<&Sent> {
        self.sending.as_ref().or(self.note.as_ref())
    }

    /// The note Joplin has under the id looked for, if its text (`md5`)
    /// is one PaperPaw wrote there: none if it was edited in Joplin.
    fn written(&self, md5: &str) -> Option<&Sent> {
        let id = &self.last()?.id;
        [&self.sending, &self.note]
            .into_iter()
            .flatten()
            .find(|note| note.id == *id && note.text == md5)
    }
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
    pub fn start(&mut self, env: &Env<'_>, start: Start, out: &mut Commands) {
        let waiting = kept::waiting(env.storage());
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
        if !env.caps().network {
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

    fn sign_in_or_check(&mut self, env: &Env<'_>, out: &mut Commands) {
        let Some(account) = Account::of(env) else {
            return self.end(env, "joplin.setup-needed", &[], End::Stop, out);
        };
        if account.service.signs_in() && self.session.is_none() {
            let request =
                service::sign_in(&account.base, account.user, account.password, &SIGNED_IN);
            return out.push(Command::http(request));
        }
        let mut request = self.request(env, INFO, "GET", &CHECKED);
        request = request.save_to(kept::file(INFO));
        out.push(Command::http(request));
    }

    /// How requests prove who sends them: the session where there is one.
    fn auth<'a>(&'a self, env: &Env<'a>) -> Auth<'a> {
        match &self.session {
            Some(session) if Service::of(env).signs_in() => Auth::Session(session),
            _ => Auth::Basic {
                user: env.settings().read(&USER).trim(),
                password: env.secrets().get(PASSWORD).unwrap_or_default(),
            },
        }
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
        let url = service.url(&service.base(env), name);
        service::request(&url, method, self.auth(env), reply)
    }

    /// A request putting the file `name`; its body is the caller's.
    fn put(&self, env: &Env<'_>, name: &str, reply: &Event<HttpReply>) -> HttpRequest {
        self.request(env, name, "PUT", reply).header(
            String::from("Content-Type"),
            String::from("application/octet-stream"),
        )
    }

    /// Answers that are no success, the same for every step.
    fn trouble(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
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
    fn unauthorised(&mut self, env: &Env<'_>, out: &mut Commands) {
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

    pub fn signed_in(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if !self.running {
            return;
        }
        match reply.status {
            200..=299 => {
                let text = reply.text();
                let body = paperpaw_sdk::json::object(&text);
                match paperpaw_sdk::json::text(&body, "id") {
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

    pub fn checked(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if !self.running {
            return;
        }
        match reply.status {
            200..=299 => {
                // Items are written as Joplin writes them for this version
                // of the format only; Joplin itself syncs with no other.
                let key = match target_info(env) {
                    (version, _) if version > item::SYNC_VERSION => "joplin.newer-target",
                    (version, _) if version < item::SYNC_VERSION => "joplin.older-target",
                    (_, true) => "joplin.encrypted",
                    _ => {
                        let service = Service::of(env);
                        let base = service.base(env);
                        let file = kept::file(LOCKS_FILE);
                        let auth = self.auth(env);
                        let request = locks::request(service, &base, auth, file, &LOCKS);
                        return out.push(Command::http(request));
                    }
                };
                self.end(env, key, &[], End::Stop, out);
            }
            404 => self.end(env, "joplin.no-data", &[], End::Stop, out),
            _ => self.trouble(env, reply, out),
        }
    }

    /// The sync target's locks are listed: nothing is written while a
    /// client upgrades it (see [`locks`]).
    pub fn locks_listed(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if !self.running {
            return;
        }
        match reply.status {
            200..=299 => {
                let now = now_ms(env).unwrap_or_default();
                let file = kept::file(LOCKS_FILE);
                let upgrading = locks::upgrading(Service::of(env), env.storage(), &file, now);
                let _ = env.storage().remove(&file);
                match upgrading {
                    Some(false) => {}
                    Some(true) => {
                        return self.end(env, "joplin.upgrading", &[], End::Again, out);
                    }
                    // Not known: nothing is written to a target that may be upgrading.
                    None => return self.end(env, "joplin.no-memory", &[], End::Again, out),
                }
                self.find_notebook(env, out);
            }
            // No locks: no folder for them (WebDAV), no lock list (a
            // Joplin Server before locks).
            404 => self.find_notebook(env, out),
            _ => self.trouble(env, reply, out),
        }
    }

    /// The notebook made before, or a new one.
    fn find_notebook(&mut self, env: &Env<'_>, out: &mut Commands) {
        match kept::notebook(env.storage()) {
            Some((name, id)) if name == notebook_name(env) && item::is_id(&id) => {
                let mut request = self.request(env, &item::file_name(&id), "GET", &NOTEBOOK_FOUND);
                request = request.save_to(kept::file(FOUND));
                request = request.with(Params::new().with(ID, id));
                out.push(Command::http(request));
            }
            _ => self.make_notebook(env, out),
        }
    }

    pub fn notebook_found(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if !self.running {
            return;
        }
        match reply.status {
            200..=299 => {
                let props = item::read_props(env.storage(), &kept::file(FOUND)).unwrap_or_default();
                let _ = env.storage().remove(&kept::file(FOUND));
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

    fn make_notebook(&mut self, env: &Env<'_>, out: &mut Commands) {
        let now = now_ms(env).unwrap_or_default();
        let name = notebook_name(env);
        let old = kept::notebook(env.storage()).map(|(_, id)| id);
        let id = item::new_id(&format!("paperpaw notebook {name} {now}"), old.as_deref());
        let mut request = self.put(env, &item::file_name(&id), &NOTEBOOK_MADE);
        request = request.body(item::notebook(
            &id,
            name,
            &UnixTime::from_millis(now).iso8601(),
        ));
        request = request.with(Params::new().with(ID, id));
        out.push(Command::http(request));
    }

    pub fn notebook_made(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if !self.running {
            return;
        }
        let Some(id) = reply.context.get(ID).filter(|_| reply.ok()) else {
            return self.trouble(env, reply, out);
        };
        let _ = kept::keep_notebook(env.storage(), notebook_name(env), id);
        self.notebook = id.to_string();
        self.next_book(env, out);
    }

    /// The next book: its note fetched if it was sent before, else made.
    fn next_book(&mut self, env: &Env<'_>, out: &mut Commands) {
        while !self.queue.is_empty() {
            let path = self.queue.remove(0);
            if let Err(StorageError::NotFound) = env.storage().size(&path) {
                // Its notes were deleted since.
                let _ = kept::remove_waiting(env.storage(), &path);
                let _ = kept::remove_sending(env.storage(), &path);
                continue;
            }
            let book = Book {
                note: kept::sent(env.storage(), &path),
                sending: kept::sending(env.storage(), &path),
                path,
            };
            let found = book.last().map(|note| item::file_name(&note.id));
            self.book = Some(book);
            let Some(found) = found else {
                return self.write_note(env, None, out);
            };
            let mut request = self.request(env, &found, "GET", &NOTE_FOUND);
            request = request.save_to(kept::file(FOUND));
            return out.push(Command::http(request));
        }
        self.finish(env, out);
    }

    pub fn note_found(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        let Some(book) = self.book.as_mut().filter(|_| self.running) else {
            return;
        };
        match reply.status {
            200..=299 => {
                let found = kept::file(FOUND);
                let props = item::read_props(env.storage(), &found).unwrap_or_default();
                // Edited in Joplin since, if its text is not what PaperPaw
                // wrote (its times tell less: moving it to another
                // notebook changes them too, some edits do not).
                let text = item::text_hash(env.storage(), &found, props.text_end).ok();
                let _ = env.storage().remove(&found);
                let written = text.and_then(|md5| book.written(&md5).cloned());
                match written {
                    Some(note) if props.kind == NOTE && !props.deleted => {
                        // Joplin has it as sent, also if the answer was lost.
                        if book.note.as_ref() != Some(&note) {
                            let _ = kept::keep_sent(env.storage(), &book.path, &note);
                            book.note = Some(note);
                        }
                        self.write_note(env, Some(props), out);
                    }
                    _ => {
                        if !props.deleted {
                            self.apart += 1;
                        }
                        self.write_note(env, None, out);
                    }
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
    fn write_note(&mut self, env: &Env<'_>, found: Option<item::Props>, out: &mut Commands) {
        let Some(mut book) = self.book.take() else {
            return;
        };
        let now = now_ms(env).unwrap_or_default();
        // Later than the note found, or Joplin keeps that one (also if
        // this device's clock is behind).
        let updated =
            UnixTime::from_millis(found.as_ref().map_or(now, |p| now.max(p.updated + 1))).iso8601();
        let old = book.last().map(|n| n.id.as_str());
        let id = match (&found, old) {
            (Some(_), Some(old)) => old.to_string(),
            _ => item::new_id(&format!("paperpaw note {} {now}", book.path), old),
        };
        let head = markdown::head(env.storage(), &book.path).unwrap_or_default();
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
        let Ok(text) = note.write(env.storage(), &book.path, &file) else {
            return self.book_failed(env, out);
        };
        let sending = Sent { id, text };
        // Kept before it goes, for when its answer is lost.
        if kept::keep_sending(env.storage(), &book.path, &sending).is_err() {
            return self.book_failed(env, out);
        }
        let mut request = self.put(env, &item::file_name(&sending.id), &NOTE_SENT);
        request = request.body_from(file);
        out.push(Command::http(request));
        book.sending = Some(sending);
        self.book = Some(book);
    }

    pub fn note_sent(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
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
        let _ = env.storage().remove(&kept::file(NOTE_FILE));
        if let Some(note) = &book.sending
            && kept::keep_sent(env.storage(), &book.path, note).is_ok()
        {
            let _ = kept::remove_sending(env.storage(), &book.path);
        }
        let _ = kept::remove_waiting(env.storage(), &book.path);
        self.sent += 1;
        self.next_book(env, out);
    }

    /// This book could not be sent: it waits, the others go on.
    fn book_failed(&mut self, env: &Env<'_>, out: &mut Commands) {
        self.book = None;
        self.failed += 1;
        self.next_book(env, out);
    }

    /// All books were tried: says how it went.
    fn finish(&mut self, env: &Env<'_>, out: &mut Commands) {
        let (sent, failed) = (self.sent as i64, self.failed as i64);
        if self.apart > 0 && !self.quiet {
            let count = self.apart as i64;
            let text = env
                .i18n()
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
            .i18n()
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
        out: &mut Commands,
    ) {
        self.end_with(env, env.i18n().format(key, args), end, out);
    }

    fn end_with(&mut self, env: &Env<'_>, text: String, end: End, out: &mut Commands) {
        self.running = false;
        self.blocked = end == End::Stop;
        self.queue.clear();
        self.book = None;
        for name in [INFO, LOCKS_FILE, FOUND, NOTE_FILE] {
            let _ = env.storage().remove(&kept::file(name));
        }
        if !self.quiet {
            out.push(Command::Notify(text.clone()));
        }
        out.push(DONE.emit(Finished { text }));
    }
}

/// What the sync target's `info.json`, just fetched, says: the version
/// of its format (0 if none) and whether its notes are end-to-end
/// encrypted.
fn target_info(env: &Env<'_>) -> (u32, bool) {
    let (mut version, mut encrypted) = (0, false);
    stream::read_file(env.storage(), &kept::file(INFO), &mut |event| {
        let Json::Value(at, value) = event else {
            return;
        };
        match at {
            [key] if key == "version" => version = value.parse().unwrap_or(0),
            [key, value_key] if key == "e2ee" && value_key == "value" => {
                encrypted = value == "true";
            }
            _ => {}
        }
    });
    (version, encrypted)
}
