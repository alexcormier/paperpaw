//! A book downloaded from a catalog: in the best format PaperPaw reads,
//! into the catalog's folder in the library, with what the catalog
//! says of it; then read at once, or later.

use super::*;

pub(super) const DOWNLOADED: Event<HttpReply> = Event::new("opds.downloaded");

/// Parameter: the file name a download gets.
pub(super) const NAME: &str = "name";

/// Parameter: the authors of a download, as the catalog names them.
pub(super) const AUTHORS: &str = "authors";

impl BrowseScreen {
    /// Downloads entry `i`'s book, or opens the feed it leads to.
    pub(super) fn open(&mut self, i: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(entry) = self.feed.entries.get(i) else {
            return;
        };
        let readable = |ext: &str| env.formats().supports(&format!("book.{ext}"));
        if let Some((book, ext)) = entry.readable_book(readable) {
            if !env.caps().network {
                let message = env.i18n().tr("opds.no-network").to_string();
                return self.say(message, cx);
            }
            let name = book_name(entry, &book.url, ext);
            let params = Params::new()
                .with(URL, book.url.clone())
                .with(NAME, name.clone())
                .with(params::TITLE, entry.title.clone())
                .with(AUTHORS, entry.authors.clone());
            cx.command(Command::http(request(
                &paperpaw_sdk::web::user_agent_of(cx.env()),
                &self.catalog,
                &book.url,
                self.temp.next("tmp"),
                &DOWNLOADED,
                params,
            )));
            let message = env
                .i18n()
                .format("opds.downloading", &[("title", Arg::Str(&entry.title))]);
            return self.say(message, cx);
        }
        if let Some(url) = entry.feed.clone() {
            return self.load(&url, false, cx);
        }
        let message = env.i18n().tr("opds.no-format").to_string();
        self.say(message, cx);
    }

    pub(super) fn downloaded(&mut self, reply: &HttpReply, file: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let url = reply.context.get(URL).unwrap_or_default().to_string();
        if !self.answered(reply, &url, cx) {
            return;
        }
        // The catalog's folder, for order on the device.
        let dir = self.catalog.sync_folder(&env);
        let name = reply.context.get(NAME).unwrap_or("download").to_string();
        if !crate::temp::may_be_book(env.storage(), file, &name) {
            let message = env.i18n().tr("opds.not-a-book").to_string();
            return self.say(message, cx);
        }
        let _ = env.storage().create_dir(&dir);
        let Ok(target) = files::put_in_free_place(env.storage(), file, &dir, &name) else {
            let message = env.i18n().tr("opds.cannot-save").to_string();
            return self.say(message, cx);
        };
        cx.command(paperpaw_sdk::library::added(&target, "opds"));
        // What the catalog says of it, for books that say nothing.
        let first = |authors: &str| authors.split(" & ").next().map(String::from);
        paperpaw_sdk::library::provide(
            env.storage(),
            &target,
            paperpaw_sdk::library::Provided::default()
                .title(reply.context.get(params::TITLE).map(String::from))
                .author(reply.context.get(AUTHORS).and_then(first)),
        );
        self.message = None;
        let text = env.i18n().format(
            "opds.saved",
            &[
                ("name", Arg::Str(files::name(&target))),
                ("folder", Arg::Str(&dir)),
            ],
        );
        cx.command(
            paperpaw_sdk::ask::choice(&text, &CHOSEN)
                .about(&target)
                .options(OPTION_LABELS, &["read", "ok"])
                .command(),
        );
        self.redraw(cx);
    }
}
