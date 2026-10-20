//! The KOReader plugin in the real kernel: opening a book takes over
//! KOReader's highlights and bookmarks at the right positions, changes
//! here are passed on, a moved book takes its KOReader folder along, and
//! hostile `metadata.epub.lua` files (garbage, deep nesting, wrong types,
//! positions nowhere, huge files) change nothing and break nothing.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_format_epub::EpubFormat;
use paperpaw_plugin_koreader::KoReader;
use paperpaw_plugin_koreader::lua::{self, Value};
use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::builtin::events::{self, AnnotationsChanged, BookMoved, Reading};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar, SDK_VERSION, Storage,
    StorageError, Version,
};
use paperpaw_zip::testing::ZipBuilder;

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 300,
        height: 400,
        dpi: 100,
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
    memory: MemoryClass::Large,
};

const BOOK: &str = "books/alice.epub";
const SIDECAR: &str = "books/alice.sdr/metadata.epub.lua";
const EXCHANGE: &str = "books/alice.sdr/alice.epub.annotations.lua";

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Alice</dc:title></metadata>
  <manifest>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#;

const CH1: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\
    <p>Alice was beginning to get very tired.</p>\
    <p>So she was considering in her own mind.</p></body></html>";
const CH2: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\
    <p>Down, down, down. Would the fall never come to an end?</p></body></html>";

/// A highlight of "Alice" with a note, and a bookmark in chapter 2.
const METADATA: &str = r#"-- we can read Lua syntax here!
return {
    ["annotations"] = {
        [1] = {
            ["datetime"] = "2026-10-01 10:00:00",
            ["drawer"] = "lighten",
            ["note"] = "Her name ✓",
            ["page"] = "/body/DocFragment[1]/body/p[1]/text().0",
            ["pos0"] = "/body/DocFragment[1]/body/p[1]/text().0",
            ["pos1"] = "/body/DocFragment[1]/body/p[1]/text().5",
            ["text"] = "Alice",
        },
        [2] = {
            ["datetime"] = "2026-10-01 10:05:00",
            ["page"] = "/body/DocFragment[2]/body/p[1]/text().6",
        },
    },
}
"#;

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

/// Counts the books whose annotations were said to have changed.
struct Listener(Rc<RefCell<Vec<String>>>);

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static("listener"),
            name: Cow::Borrowed("Listener"),
            description: Cow::Borrowed(""),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NOTHING,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let heard = self.0.clone();
        r.on(events::ANNOTATIONS_CHANGED, move |_, changed, _| {
            heard.borrow_mut().push(changed.path.clone())
        })
    }
}

fn epub() -> Vec<u8> {
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", OPF)
        .deflated("OEBPS/ch1.xhtml", CH1)
        .deflated("OEBPS/ch2.xhtml", CH2)
        .build()
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    changed: Rc<RefCell<Vec<String>>>,
    now: u64,
}

impl Device {
    /// The book, and KOReader's file `metadata` beside it if given.
    fn new(metadata: Option<&[u8]>) -> Self {
        let storage = Shared::default();
        storage.write(BOOK, &epub()).unwrap();
        if let Some(metadata) = metadata {
            storage.write(SIDECAR, metadata).unwrap();
        }
        let changed = Rc::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .plugin(EpubFormat)
            .plugin(KoReader)
            .plugin(Listener(Rc::clone(&changed)))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            storage,
            changed,
            now: 0,
        }
    }

    fn send<T: EventData>(&mut self, event: paperpaw_sdk::event::Event<T>, data: &T) {
        self.now += 100;
        self.kernel
            .send_event(Instant(self.now), event.id().clone(), data.to_params());
        // What was read is kept until a frame is drawn.
        self.kernel.render(&mut |_, _| {});
    }

    fn open(&mut self) {
        let reading = Reading::new(
            BOOK.to_string(),
            String::from("Alice"),
            0.0,
            0,
            0,
            String::new(),
        );
        self.send(events::BOOK_OPENED, &reading);
    }

    fn ours(&self) -> Vec<Annotation> {
        annotations::load(&self.storage, BOOK)
    }

    /// The annotations in the file for KOReader.
    fn passed(&self) -> Vec<lua::Table> {
        let Some(data) = self.storage.0.get(EXCHANGE) else {
            return Vec::new();
        };
        let Some(Value::Table(root)) = lua::parse(&String::from_utf8(data).unwrap()) else {
            panic!("not Lua");
        };
        let Some(Value::Table(items)) = root.get("annotations") else {
            panic!("no annotations");
        };
        items
            .list()
            .map(|v| match v {
                Value::Table(t) => t.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn locate(&self, xpointer: &str) -> paperpaw_sdk::document::Locator {
        let doc = self.kernel.formats().open(&self.storage, BOOK).unwrap();
        doc.locate_xpointer(&self.storage, xpointer).unwrap()
    }
}

#[test]
fn opening_a_book_takes_over_koreaders_annotations_where_they_are() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device.open();
    let ours = device.ours();
    assert_eq!(ours.len(), 2, "{ours:?}");
    let highlight = ours
        .iter()
        .find(|a| a.mark.kind == Kind::Highlight)
        .unwrap();
    assert_eq!(
        highlight.mark.start,
        device.locate("/body/DocFragment[1]/body/p[1]/text().0")
    );
    assert_eq!(
        highlight.mark.end,
        device.locate("/body/DocFragment[1]/body/p[1]/text().5")
    );
    assert_eq!(highlight.text, "Alice");
    assert_eq!(highlight.note, "Her name ✓");
    assert!(highlight.mark.noted);
    let bookmark = ours.iter().find(|a| a.mark.kind == Kind::Bookmark).unwrap();
    assert_eq!(
        bookmark.mark.start,
        device.locate("/body/DocFragment[2]/body/p[1]/text().6")
    );
    assert_eq!(bookmark.mark.start.section, 1);
    // In the second of two chapters of about the same length.
    assert!(bookmark.percent >= 40, "{}", bookmark.percent);
    assert_eq!(device.changed.borrow().as_slice(), [BOOK]);
    // Nothing new for KOReader.
    assert!(!device.storage.exists(EXCHANGE));

    // Opened again: nothing changes, nothing is said.
    device.open();
    assert_eq!(device.ours().len(), 2);
    assert_eq!(device.changed.borrow().len(), 1);
}

#[test]
fn changes_here_are_passed_on_to_koreader() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device.open();
    let start = device.locate("/body/DocFragment[1]/body/p[2]/text().3");
    let end = device.locate("/body/DocFragment[1]/body/p[2]/text().6");
    annotations::add(
        &device.storage,
        BOOK,
        Annotation {
            mark: Mark {
                kind: Kind::Highlight,
                start,
                end,
                noted: true,
            },
            percent: 20,
            created: 1_792_000_000,
            updated: 0,
            text: String::from("she"),
            note: String::from("Quote \"this\"\\"),
        },
    );
    let changed = AnnotationsChanged::new(BOOK.to_string());
    device.send(events::ANNOTATIONS_CHANGED, &changed);
    let passed = device.passed();
    assert_eq!(passed.len(), 3, "{passed:?}");
    let new = passed
        .iter()
        .find(|i| i.str("text") == Some("she"))
        .unwrap();
    assert_eq!(new.str("note"), Some("Quote \"this\"\\"));
    assert_eq!(new.str("drawer"), Some("lighten"));
    assert_eq!(device.locate(new.str("pos0").unwrap()), start);
    assert_eq!(device.locate(new.str("pos1").unwrap()), end);
}

#[test]
fn books_without_a_koreader_folder_are_left_alone() {
    let mut device = Device::new(None);
    device.open();
    assert!(device.ours().is_empty());
    assert!(device.changed.borrow().is_empty());
    let mut written = Vec::new();
    let _ = device.storage.list(".paperpaw/plugins/koreader", &mut |e| {
        written.push(e.name.to_string())
    });
    assert!(written.is_empty(), "{written:?}");
}

#[test]
fn hostile_koreader_files_change_nothing() {
    let huge = format!(
        "return {{ annotations = {{ {{ text = \"{}\" }} }} }}",
        "x".repeat(5 << 20)
    );
    let files: Vec<Vec<u8>> = vec![
        b"garbage".to_vec(),
        vec![0xff, 0xfe, 0x00, 0x80],
        format!("return {}{}", "{".repeat(10_000), "}".repeat(10_000)).into_bytes(),
        b"return { annotations = \"not a table\" }".to_vec(),
        b"return { annotations = { 1, true, \"x\", 0x7fffffffffffffffff } }".to_vec(),
        // Wrong types where text is expected.
        br#"return { annotations = { { drawer = "lighten", pos0 = 5, pos1 = {}, text = 1 },
            { page = true }, { page = { "/body" } } } }"#
            .to_vec(),
        // Positions that are nowhere, or backwards.
        br#"return { annotations = {
            { page = "/body/DocFragment[99]/body/p[1]/text().0" },
            { page = "not an xpointer \u{0}" },
            { drawer = "lighten", pos0 = "/body/DocFragment[1]/body/p[1]/text().5",
              pos1 = "/body/DocFragment[1]/body/p[1]/text().0" },
            { drawer = "lighten", pos0 = "/body/DocFragment[1]/body/p[1]/text().99999999999",
              pos1 = "/body/DocFragment[1]/body/p[1]/text().999999999999999" } } }"#
            .to_vec(),
        huge.into_bytes(),
    ];
    for file in files {
        let label = String::from_utf8_lossy(&file[..file.len().min(60)]).into_owned();
        let mut device = Device::new(Some(&file));
        device.open();
        assert!(device.ours().is_empty(), "{label}");
        assert!(device.changed.borrow().is_empty(), "{label}");
        assert!(!device.storage.exists(EXCHANGE), "{label}");
        // KOReader's own file is never written.
        assert_eq!(device.storage.0.get(SIDECAR).unwrap(), file, "{label}");
    }
}

#[test]
fn odd_dates_and_fields_are_read_without_harm() {
    let file = r#"return { annotations = {
        { datetime = "99999-99-99 99:99:99", drawer = "lighten",
          pos0 = "/body/DocFragment[1]/body/p[1]/text().0",
          pos1 = "/body/DocFragment[1]/body/p[1]/text().5",
          note = "line one\nline two", text = "Ålice ✓",
          unknown_field = { nested = { deeper = 1 } } },
        { datetime = "-001-01-01", page = "/body/DocFragment[1]/body/p[2]/text().0" },
        { datetime = "", datetime_updated = "2026-02-30 25:61:61",
          page = "/body/DocFragment[2]/body/p[1]/text().0" },
    } }"#;
    let mut device = Device::new(Some(file.as_bytes()));
    device.open();
    let ours = device.ours();
    assert_eq!(ours.len(), 3, "{ours:?}");
    let highlight = ours
        .iter()
        .find(|a| a.mark.kind == Kind::Highlight)
        .unwrap();
    assert_eq!(highlight.text, "Ålice ✓");
    // Notes are one line here.
    assert!(!highlight.note.contains('\n'), "{:?}", highlight.note);
    assert!(highlight.note.contains("line one") && highlight.note.contains("line two"));
}

#[test]
fn what_koreader_has_and_paperpaw_does_not_know_is_passed_on_unchanged() {
    // An annotation PaperPaw cannot place, with fields of its own, beside
    // a new one here: written back exactly as KOReader had it.
    let file = r#"return { annotations = {
        { drawer = "underscore", color = "yellow", pos0 = "/body/DocFragment[9]/p[1]/text().0",
          pos1 = "/body/DocFragment[9]/p[1]/text().3", text = "Elsewhere",
          extra = { 1, 2, { three = true } }, weight = -3.5e-2 },
    } }"#;
    let Some(Value::Table(root)) = lua::parse(file) else {
        panic!("not Lua");
    };
    let Some(Value::Table(items)) = root.get("annotations") else {
        panic!("no annotations");
    };
    let theirs = items.list().next().unwrap().clone();
    let mut device = Device::new(Some(file.as_bytes()));
    let start = device.locate("/body/DocFragment[1]/body/p[1]/text().0");
    annotations::add(
        &device.storage,
        BOOK,
        Annotation {
            mark: Mark {
                kind: Kind::Bookmark,
                start,
                end: start,
                noted: false,
            },
            percent: 0,
            created: 1_792_000_000,
            updated: 0,
            text: String::new(),
            note: String::new(),
        },
    );
    device.open();
    let passed = device.passed();
    assert_eq!(passed.len(), 2);
    assert_eq!(Value::Table(passed[0].clone()), theirs);
    assert!(passed[1].get("drawer").is_none());
}

#[test]
fn a_moved_book_takes_its_koreader_folder_along() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device
        .storage
        .write(EXCHANGE, b"return { annotations = {} }")
        .unwrap();
    device
        .storage
        .write("books/new/wonder.epub", &epub())
        .unwrap();
    device.storage.remove(BOOK).unwrap();
    let moved = BookMoved::new(BOOK.to_string(), String::from("books/new/wonder.epub"));
    device.send(events::BOOK_MOVED, &moved);
    assert!(!device.storage.exists(SIDECAR));
    assert_eq!(
        device
            .storage
            .0
            .get("books/new/wonder.sdr/metadata.epub.lua")
            .unwrap(),
        METADATA.as_bytes()
    );
    // KOReader's pending file is named after the book: renamed too.
    assert!(
        device
            .storage
            .exists("books/new/wonder.sdr/wonder.epub.annotations.lua")
    );
    assert!(
        !device
            .storage
            .exists("books/new/wonder.sdr/alice.epub.annotations.lua")
    );
}
