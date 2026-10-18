//! Browsing a catalog with the real kernel: its feed is loaded and shown,
//! an entry leads to a further feed and *Back* to the one before, a book
//! is downloaded into the catalog's folder in the library, and what goes
//! wrong is said.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::rc::Rc;

use quireink_core::{HttpCall, HttpResponse, Io, Kernel};
use quireink_platform::Rotation;
use quireink_plugin_opds::{OPEN, Opds};
use quireink_sdk::builtin::actions;
use quireink_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Small,
};

/// The catalog the list starts with.
const START: &str = "https://www.gutenberg.org/ebooks.opds/";

const FEED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Project Gutenberg</title>
<link rel="search" type="application/atom+xml" href="/ebooks/search.opds/?query={searchTerms}"/>
<link rel="next" type="application/atom+xml;profile=opds-catalog" href="/ebooks.opds/?page=2"/>
<entry>
  <title>Popular books</title>
  <link rel="subsection" type="application/atom+xml;profile=opds-catalog" href="/ebooks/popular.opds/"/>
</entry>
<entry>
  <title>Alice's Adventures in Wonderland</title>
  <author><name>Lewis Carroll</name></author>
  <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/ebooks/11.epub"/>
</entry>
</feed>"#;

const POPULAR: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Popular books</title>
<entry>
  <title>Die Verwandlung</title>
  <author><name>Franz Kafka</name></author>
  <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/ebooks/5200.epub"/>
</entry>
</feed>"#;

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

struct Browsing {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Browsing {
    /// The first catalog opened: its feed is asked for.
    fn new() -> (Self, HttpCall) {
        let storage = Shared::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .network(true)
            .plugin(quireink_plugin_fonts_default::DefaultFontPacks)
            .plugin(quireink_plugin_format_epub::EpubFormat)
            .plugin(Opds)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut browsing = Self {
            kernel,
            storage,
            now: 0,
        };
        browsing.run(OPEN);
        browsing.run(actions::CONFIRM);
        let call = browsing.request();
        assert_eq!(call.url, START);
        (browsing, call)
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The one request the kernel asks the shell to make now.
    fn request(&mut self) -> HttpCall {
        let mut calls: Vec<HttpCall> = self
            .kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls.remove(0)
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &str) {
        self.now += 100;
        let response = HttpResponse {
            id: call.id,
            status,
            body: body.as_bytes().to_vec(),
            ..HttpResponse::default()
        };
        self.kernel.http_response(Instant(self.now), response);
    }

    /// The labels of what the screen shows, in order.
    fn shown(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }
}

#[test]
fn a_feed_is_shown_with_search_and_more() {
    let (mut browsing, call) = Browsing::new();
    browsing.answer(&call, 200, FEED);
    let shown = browsing.shown();
    let at = |text: &str| shown.iter().position(|l| l.contains(text));
    let (search, popular, alice) = (at("Search"), at("Popular books"), at("Alice"));
    assert!(
        search.is_some() && popular.is_some() && alice.is_some(),
        "{shown:?}"
    );
    assert!(search < popular && popular < alice, "{shown:?}");
    assert!(at("Lewis Carroll").is_some(), "{shown:?}");
    assert!(at("More").is_some(), "{shown:?}");
}

#[test]
fn entries_lead_on_and_back() {
    let (mut browsing, call) = Browsing::new();
    browsing.answer(&call, 200, FEED);
    // Past *Search*, to *Popular books*.
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    // Relative to the feed it came in.
    assert_eq!(call.url, "https://www.gutenberg.org/ebooks/popular.opds/");
    browsing.answer(&call, 200, POPULAR);
    assert!(
        browsing
            .shown()
            .iter()
            .any(|l| l.contains("Die Verwandlung"))
    );
    // Back: the first feed again.
    browsing.run(actions::BACK);
    let call = browsing.request();
    assert_eq!(call.url, START);
    browsing.answer(&call, 200, FEED);
    assert!(browsing.shown().iter().any(|l| l.contains("Popular books")));
}

#[test]
fn a_book_is_downloaded_into_the_library() {
    let (mut browsing, call) = Browsing::new();
    browsing.answer(&call, 200, FEED);
    // Search, Popular books, then the book.
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    assert_eq!(call.url, "https://www.gutenberg.org/ebooks/11.epub");
    assert!(call.save_to.is_some());
    browsing.answer(&call, 200, "PK the book");
    // In the catalog's folder in the library (here, without the files
    // plugin, the card's top), named after the book.
    let mut saved = Vec::new();
    browsing
        .storage
        .list("Project Gutenberg", &mut |entry| {
            saved.push(entry.name.to_string());
        })
        .unwrap();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].ends_with(".epub"), "{saved:?}");
    let book = format!("Project Gutenberg/{}", saved[0]);
    assert_eq!(browsing.storage.0.get(&book).unwrap(), b"PK the book");
}

#[test]
fn what_goes_wrong_is_said() {
    let (mut browsing, call) = Browsing::new();
    browsing.answer(&call, 401, "");
    let shown = browsing.shown().join(" | ");
    assert!(
        shown.contains("password") || shown.contains("allowed"),
        "{shown}"
    );

    let (mut browsing, call) = Browsing::new();
    browsing.answer(&call, 200, "<html><body>Not a feed</body></html>");
    let shown = browsing.shown().join(" | ");
    assert!(shown.to_lowercase().contains("catalog"), "{shown}");

    let (mut browsing, call) = Browsing::new();
    browsing.answer(&call, 0, "no network");
    let shown = browsing.shown().join(" | ");
    assert!(shown.contains("gutenberg.org"), "{shown}");
}
