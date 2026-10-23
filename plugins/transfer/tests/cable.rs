//! Transfer over the cable (USB) with the real kernel: lines as the
//! computer sends them (`Kernel::cable_line`) and the answers as the
//! device sends them back. A computer that speaks opens the screen; it
//! learns what the device is without asking the reader, but nothing is
//! written before the reader allowed what it offered, and only that, at
//! its size and digest; the plugin is installed in one step, books never
//! go over others; Back ends the permission; the radio stays off.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;
use std::rc::Rc;

use paperpaw_core::{CodeFile, CodeLoader, Effects, Io, Kernel};
use paperpaw_native::{Chip, Header, LINK_BASE};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_transfer::Transfer;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::native::{Fingerprint, Native, installed_code};
use paperpaw_sdk::{
    DirEntry, MemoryStorage, Plugin, PluginProblem, Storage, StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
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

const BUILD: Fingerprint = Fingerprint(0x0123_4567_89ab_cdef);

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
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
}

struct NoCode;

impl CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Damaged)
    }
}

fn sha(data: &[u8]) -> String {
    paperpaw_digest::sha256::hex(data)
}

fn ppn() -> Vec<u8> {
    let image = [0u8; 96];
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: BUILD.0,
        base: LINK_BASE,
        image_len: image.len() as u32,
        code_at: 64,
        entry: 0,
        api: 16,
        relocs: 0,
        align: 16,
        sha256: [0; 32],
    };
    let mut digest = header.digest();
    digest.update(&image);
    header.sha256 = digest.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    file
}

struct Computer {
    kernel: Kernel,
    storage: Shared,
    now: u64,
    next: u32,
}

impl Computer {
    fn new(storage: Shared) -> Self {
        let mut native = Native::new("code.ppn");
        native.fingerprint = BUILD;
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .user_config("[core]\nlanguage = en\n")
            .network(true)
            .wifi(true)
            .code_plugins(native, Box::new(NoCode))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Transfer)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut computer = Self {
            kernel,
            storage,
            now: 0,
            next: 1,
        };
        computer.settle(effects);
        computer
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn line(&mut self, line: &str) {
        self.now += 10;
        let effects = self.kernel.cable_line(Instant(self.now), line);
        self.settle(effects);
    }

    /// A request over the cable; the status and body of the answer.
    fn request(&mut self, method: &str, target: &str, body: &[u8]) -> (u16, String) {
        let n = self.next;
        self.next += 1;
        self.line(&format!("PPT/1 {n} {method} {target} {}", body.len()));
        for part in body.chunks(512) {
            let hex: String = part.iter().map(|b| format!("{b:02x}")).collect();
            self.line(&format!("PPT/1 {n} data {hex}"));
        }
        let answers = answers(&self.kernel.take_cable_lines());
        answers
            .get(&n)
            .cloned()
            .unwrap_or_else(|| panic!("no answer to {method} {target}: {answers:?}"))
    }

    fn put_plugin(&mut self, id: &str, path: &str, data: &[u8]) -> u16 {
        let target = format!("/api/plugin/{id}/{path}?sha256={}", sha(data));
        self.request("PUT", &target, data).0
    }

    /// Whether the screen says `text` (lines joined: a long text wraps).
    fn says(&mut self, text: &str) -> bool {
        self.kernel.render(&mut |_, _| {});
        let labels: Vec<String> = self
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        labels.join(" ").contains(text)
    }

    fn press(&mut self, action: paperpaw_sdk::Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// The reader answers the question shown: allow, or not.
    fn allow(&mut self, yes: bool) {
        if yes {
            self.press(actions::LEFT);
        }
        self.press(actions::CONFIRM);
    }

    fn files_below(&self, dir: &str) -> usize {
        let mut count = 0;
        let _ = self.storage.list(dir, &mut |_| count += 1);
        count
    }
}

/// The answers among `lines`, by the computer's numbers: status and body.
fn answers(lines: &[String]) -> BTreeMap<u32, (u16, String)> {
    let mut answers: BTreeMap<u32, (u16, Vec<u8>)> = BTreeMap::new();
    for line in lines {
        let rest = line.strip_prefix("PPT/1 ").expect(line);
        let mut words = rest.split(' ');
        let n: u32 = words.next().unwrap().parse().unwrap();
        match words.next().unwrap() {
            "data" => {
                let hex = words.next().unwrap();
                let bytes = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
                answers.get_mut(&n).unwrap().1.extend(bytes);
            }
            "end" => {}
            status => {
                answers.insert(n, (status.parse().unwrap(), Vec::new()));
            }
        }
    }
    answers
        .into_iter()
        .map(|(n, (status, body))| (n, (status, String::from_utf8(body).unwrap())))
        .collect()
}

const INI: &str = "[plugin]\nid = airy\nname = Airy\nversion = 1.0.0\n";

#[test]
fn a_computer_that_speaks_opens_the_screen_and_learns_what_the_device_is() {
    let mut computer = Computer::new(Shared::default());
    let (status, body) = computer.request("GET", "/api/hello", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""protocol":1"#), "{body}");
    assert!(body.contains(r#""device":"test-x4""#), "{body}");
    assert!(
        body.contains(r#""fingerprint":"0123456789abcdef""#),
        "{body}"
    );
    assert_eq!(
        computer.kernel.active_screen().map(|s| s.as_str()),
        Some("transfer.cable")
    );
    assert!(computer.says("connected over USB"));
    let (status, body) = computer.request("GET", "/api/plugins", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""id":"transfer""#), "{body}");
    // The radio is not asked for: the cable needs no network.
    let radio = computer
        .kernel
        .take_io()
        .into_iter()
        .any(|io| matches!(io, Io::Wifi(_)));
    assert!(!radio);
    // Lines of others (a log, a remote control) are passed over.
    computer.line("PP:OK");
    assert!(computer.kernel.take_cable_lines().is_empty());
}

#[test]
fn nothing_is_written_before_the_reader_allows_it() {
    let mut computer = Computer::new(Shared::default());
    let book = b"a book";
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    let offer = format!("book a.epub {}\n", book.len());
    let (status, body) = computer.request("POST", "/api/offer", offer.as_bytes());
    assert_eq!((status, body.as_str()), (202, r#"{"state":"asking"}"#));
    assert!(computer.says("Allow this computer to add books?"));
    assert!(computer.says("a.epub ("));
    // Asking: still nothing.
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    computer.allow(false);
    assert_eq!(
        computer.request("GET", "/api/offer", b"").1,
        r#"{"state":"refused"}"#
    );
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    assert_eq!(computer.files_below("books"), 0);
    // Offers that name what may not be written are no offers.
    for bad in [
        "book ../a.epub 3\n",
        "book .hidden 3\n",
        "plugin ../x\nplugin.ini 3\n",
        "plugin airy\n../paperpaw.ini 3\n",
        "settings 3\n",
    ] {
        let (status, _) = computer.request("POST", "/api/offer", bad.as_bytes());
        assert_eq!(status, 400, "{bad}");
    }
}

#[test]
fn what_was_allowed_comes_in_whole_and_nothing_else() {
    let storage = Shared::default();
    storage.write("books/a.epub", b"the reader's own").unwrap();
    let mut computer = Computer::new(storage);
    computer.request("GET", "/api/hello", b"");
    let book = b"a new book";
    let offer = format!(
        "plugin airy\nplugin.ini {}\nbook a.epub {}\n",
        INI.len(),
        book.len()
    );
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    assert!(computer.says("install a plugin and add books"));
    computer.allow(true);
    assert!(computer.says("Allowed"));
    // Only what was offered, at its size and digest.
    assert_eq!(computer.put_plugin("airy", "i18n/de.ini", b"[meta]"), 403);
    assert_eq!(
        computer.put_plugin("other", "plugin.ini", INI.as_bytes()),
        403
    );
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}", sha(b"x"));
    let wrong = format!("{}{}", &INI[..INI.len() - 1], "x");
    assert_eq!(computer.request("PUT", &target, wrong.as_bytes()).0, 422);
    assert_eq!(
        computer.put_plugin("airy", "plugin.ini", INI.as_bytes()),
        200
    );
    let (status, body) = computer.request("POST", "/api/plugin/airy", b"");
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        computer
            .kernel
            .plugins()
            .get("airy")
            .map(|p| p.manifest.version),
        Some(Version::new(1, 0, 0))
    );
    // The book under a free name: the reader's own stays.
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    let (status, body) = computer.request("PUT", &target, book);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        computer.storage.0.get("books/a.epub").unwrap(),
        b"the reader's own"
    );
    assert!(body.contains("books/a"), "{body}");
    // Offered once, taken once.
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    assert_eq!(computer.request("POST", "/api/plugin/airy", b"").0, 403);
    assert!(!body.contains(r#""books/a.epub""#), "{body}");
    assert!(body.contains(".epub"), "{body}");
    let mut left = 0;
    let _ = computer.storage.list(".paperpaw/tmp", &mut |_| left += 1);
    assert_eq!(left, 0, "a body left behind");
}

#[test]
fn code_runs_as_the_reader_allowed_and_back_ends_the_permission() {
    let mut computer = Computer::new(Shared::default());
    let code = ppn();
    let ini = "[plugin]\nid = clock\nname = Clock\nversion = 1.0.0\n";
    let offer = format!(
        "plugin clock\nplugin.ini {}\ncode.ppn {}\n",
        ini.len(),
        code.len()
    );
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    assert!(computer.says("program code from this computer"));
    computer.allow(true);
    assert_eq!(
        computer.put_plugin("clock", "plugin.ini", ini.as_bytes()),
        200
    );
    assert_eq!(computer.put_plugin("clock", "code.ppn", &code), 200);
    let (status, body) = computer.request("POST", "/api/plugin/clock", b"");
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(r#""restart":true"#), "{body}");
    assert!(installed_code(&computer.storage, "clock").is_some());
    // Back: the permission ends; the next request asks anew.
    computer.press(actions::BACK);
    assert_ne!(
        computer.kernel.active_screen().map(|s| s.as_str()),
        Some("transfer.cable")
    );
    assert_eq!(
        computer.put_plugin("clock", "plugin.ini", ini.as_bytes()),
        403
    );
    assert_eq!(
        computer.kernel.active_screen().map(|s| s.as_str()),
        Some("transfer.cable")
    );
}
