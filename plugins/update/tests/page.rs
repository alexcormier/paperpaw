//! The update page with the real kernel: an update that leaves out a
//! plugin built in says which, an update named by the update server is
//! downloaded into `updates` and then offered like one copied there, and
//! the server's other answers are said.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_update::{CHECK, OPEN, Update};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::firmware::{Firmware, PLUGINS_AT, PluginCrate, Plugins};
use paperpaw_sdk::{
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

const SEED: [u8; 32] = [3; 32];
const CHIP: u16 = paperpaw_sdk::firmware::CHIP_ESP32_C3;
const PROJECT: &str = "paperpaw-xteink";
/// What the running firmware has built in: a plugin of its own beside
/// PaperPaw's.
const RUNNING: &str = "paperpaw-plugin-stats 0.4.0\nchess-for-paperpaw 1.0.0\n";

/// A signed update file of `version` with the plugin crates `plugins`.
fn update_file(version: &str, plugins: &[(&str, &str)]) -> Vec<u8> {
    let mut image = vec![0u8; 2000];
    image[0] = 0xE9;
    image[12..14].copy_from_slice(&CHIP.to_le_bytes());
    image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
    image[48..48 + version.len()].copy_from_slice(version.as_bytes());
    image[80..80 + PROJECT.len()].copy_from_slice(PROJECT.as_bytes());
    let list = Plugins::record(
        plugins
            .iter()
            .map(|&(name, version)| PluginCrate { name, version }),
    );
    image[PLUGINS_AT..PLUGINS_AT + list.len()].copy_from_slice(&list);
    let trailer = paperpaw_update::sign(&image, &SEED).unwrap();
    image.extend_from_slice(&trailer.to_bytes());
    image
}

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

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Device {
    /// Running PaperPaw 0.4.0 with [`RUNNING`], the card as `storage` has
    /// it.
    fn new(storage: Shared) -> Self {
        let firmware = Firmware {
            version: "0.4.0".into(),
            project: PROJECT.into(),
            chip: CHIP,
            keys: vec![paperpaw_update::public_key(&SEED)],
            room: 1_000_000,
            plugins: Plugins::new(RUNNING),
            ..Firmware::default()
        };
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .network(true)
            .firmware(firmware)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Update)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            storage,
            now: 0,
        }
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

    fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) {
        self.now += 100;
        let response = HttpResponse {
            id: call.id,
            status,
            body: body.to_vec(),
            ..HttpResponse::default()
        };
        self.kernel.http_response(Instant(self.now), response);
    }

    /// What the screen shows, as one text.
    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// The server's answer naming PaperPaw 0.5.0 without the reader's own
/// plugin.
fn offer(size: usize) -> String {
    format!(
        r#"{{"version": "0.5.0", "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
            "size": {size}, "plugins": ["paperpaw-plugin-stats 0.5.0"],
            "missing": ["chess-for-paperpaw not-ready"]}}"#
    )
}

#[test]
fn an_update_on_the_card_says_what_it_leaves_out() {
    let storage = Shared::default();
    storage
        .write(
            "updates/paperpaw-x4-0.5.0.bin",
            &update_file("0.5.0", &[("paperpaw-plugin-stats", "0.5.0")]),
        )
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0"), "{shown}");
    assert!(
        shown.contains("cannot come along: chess-for-paperpaw."),
        "{shown}"
    );
    assert!(shown.contains("Update without them"), "{shown}");
}

#[test]
fn an_update_with_every_plugin_says_nothing_of_them() {
    let storage = Shared::default();
    storage
        .write(
            "updates/paperpaw-x4-0.5.0.bin",
            &update_file(
                "0.5.0",
                &[
                    ("chess-for-paperpaw", "1.1.0"),
                    ("paperpaw-plugin-stats", "0.5.0"),
                ],
            ),
        )
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    assert!(!shown.contains("cannot come along"), "{shown}");
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn an_update_from_the_server_is_downloaded_and_offered() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    assert!(device.shown().contains("Search online"));
    device.run(actions::CONFIRM);
    let call = device.request();
    assert_eq!(call.method, "POST");
    assert_eq!(call.url, "https://paperpaw.org/api/v1/updates");
    let question = String::from_utf8(call.body.clone().unwrap_or_default()).unwrap();
    assert!(question.contains(r#""version":"0.4.0""#), "{question}");
    assert!(
        question.contains(r#""chess-for-paperpaw 1.0.0""#),
        "{question}"
    );
    assert!(device.shown().contains("Searching for an update"));

    let file = update_file("0.5.0", &[("paperpaw-plugin-stats", "0.5.0")]);
    device.answer(&call, 200, offer(file.len()).as_bytes());
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    assert!(
        shown.contains("chess-for-paperpaw (not ready yet)"),
        "{shown}"
    );
    assert!(shown.contains("Download without them"), "{shown}");

    device.run(actions::CONFIRM);
    let call = device.request();
    assert_eq!(call.method, "GET");
    assert_eq!(
        call.url,
        "https://paperpaw.org/api/v1/updates/paperpaw-x4-0.5.0-update.bin"
    );
    assert!(device.shown().contains("Downloading PaperPaw 0.5.0"));
    device.answer(&call, 200, &file);
    assert_eq!(
        device
            .storage
            .0
            .get("updates/paperpaw-xteink-0.5.0.bin")
            .as_deref(),
        Some(file.as_slice())
    );
    let shown = device.shown();
    assert!(shown.contains("Update without them"), "{shown}");
}

#[test]
fn a_download_cut_short_is_not_kept() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, b"only a part");
    let mut kept = Vec::new();
    let _ = device.storage.list("updates", &mut |entry| {
        kept.push(entry.name.to_string());
    });
    assert!(kept.is_empty(), "{kept:?}");
    // Offered again, to try once more.
    assert!(device.shown().contains("PaperPaw 0.5.0 is available"));
}

#[test]
fn the_server_knows_nothing_newer() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, br#"{"version": ""}"#);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw is up to date"), "{shown}");
}

#[test]
fn no_answer_is_said() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 0, b"no network");
    let shown = device.shown();
    assert!(shown.contains("No answer"), "{shown}");
    assert!(shown.contains("(no network)"), "{shown}");
    assert!(shown.contains("Try again"), "{shown}");
}

#[test]
fn an_error_of_the_server_is_said() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 404, b"not found");
    let shown = device.shown();
    assert!(
        shown.contains("answered with an error (HTTP 404)"),
        "{shown}"
    );
}

#[test]
fn a_search_in_the_background_offers_what_it_finds_once() {
    let mut device = Device::new(Shared::default());
    device.run(CHECK);
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    // Not offered again by the next search.
    device.run(actions::BACK);
    device.run(CHECK);
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    assert!(!device.shown().contains("is available"));
}

/// Loads no code: the test looks at what is fetched, not at what runs.
struct NoCode;

impl paperpaw_core::CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_sdk::Plugin>, paperpaw_sdk::PluginProblem> {
        Err(paperpaw_sdk::PluginProblem::Unsupported)
    }
}

/// The statistics plugin built into the running firmware (by its id).
struct BuiltInStats;

impl paperpaw_sdk::Plugin for BuiltInStats {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest {
            id: Id::new_static("stats"),
            name: "Statistics".into(),
            description: "Reading statistics.".into(),
            version: paperpaw_sdk::Version::new(0, 4, 0),
            sdk: paperpaw_sdk::SDK_VERSION,
        }
    }

    fn register(&self, _: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        Ok(())
    }
}

/// The author key of the plugin files.
const AUTHOR: [u8; 32] = [7; 32];

/// A plugin file built for the next firmware, signed by [`AUTHOR`].
fn code_file() -> Vec<u8> {
    use paperpaw_sdk::native::{Chip, Header, signature};
    let mut header = Header {
        chip: Chip::from_id(5).unwrap(),
        fingerprint: 0x5eed,
        base: 0x3c00_0000,
        image_len: 128,
        code_at: 64,
        entry: 0,
        api: 16,
        relocs: 0,
        align: 16,
        sha256: [0; 32],
    };
    let image = [0u8; 128];
    let mut digest = header.digest();
    digest.update(&image);
    header.sha256 = digest.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    let trailer = signature::sign(&file, &AUTHOR);
    file.extend_from_slice(&trailer.to_bytes());
    file
}

const PLUGIN_INI: &str = "[plugin]\nid = stats\nname = Statistics\nversion = 0.5.0\n";

/// The server's answer naming PaperPaw 0.5.0, which has the statistics
/// as a file instead of built in.
fn offer_with_code(size: usize, code: &[u8]) -> String {
    format!(
        r#"{{"version": "0.5.0", "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
            "size": {size}, "plugins": ["chess-for-paperpaw 1.1.0"],
            "code": [{{"id": "stats", "name": "Statistics", "version": "0.5.0",
              "author_key": "{author}",
              "files": [
                {{"path": "plugin.ini", "url": "https://p.org/stats/plugin.ini",
                  "size": {ini_size}, "sha1": "{ini_sha1}"}},
                {{"path": "code.ppn", "url": "https://p.org/stats/x4/code.ppn",
                  "size": {code_size}, "sha256": "{code_sha256}"}}]}}]}}"#,
        author =
            paperpaw_digest::hex::encode(&paperpaw_sdk::native::signature::public_key(&AUTHOR)),
        ini_size = PLUGIN_INI.len(),
        ini_sha1 = paperpaw_digest::sha1::hex(PLUGIN_INI.as_bytes()),
        code_size = code.len(),
        code_sha256 = paperpaw_digest::sha256::hex(code),
    )
}

#[test]
fn plugins_that_become_files_are_fetched_before_the_update() {
    let storage = Shared::default();
    let firmware = Firmware {
        version: "0.4.0".into(),
        project: PROJECT.into(),
        chip: CHIP,
        keys: vec![paperpaw_update::public_key(&SEED)],
        room: 1_000_000,
        plugins: Plugins::new(RUNNING),
        ..Firmware::default()
    };
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .network(true)
        .firmware(firmware)
        .code_plugins(
            paperpaw_sdk::native::Native::new("code.ppn"),
            Box::new(NoCode),
        )
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(BuiltInStats)
        .plugin(Update)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let mut device = Device {
        kernel,
        storage,
        now: 0,
    };
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    let file = update_file("0.5.0", &[("chess-for-paperpaw", "1.1.0")]);
    let code = code_file();
    device.answer(&call, 200, offer_with_code(file.len(), &code).as_bytes());
    let shown = device.shown();
    // The statistics come back as a file: not named as left out.
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    assert!(!shown.contains("cannot come along"), "{shown}");

    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, &file);
    // Then the plugin's files, one after another; the update waits.
    let call = device.request();
    assert_eq!(call.url, "https://p.org/stats/plugin.ini");
    assert!(device.shown().contains("Downloading PaperPaw 0.5.0"));
    device.answer(&call, 200, PLUGIN_INI.as_bytes());
    let call = device.request();
    assert_eq!(call.url, "https://p.org/stats/x4/code.ppn");
    device.answer(&call, 200, &code);

    // Where the updated firmware installs it, its code named as fetched.
    let storage = &device.storage;
    assert!(storage.exists("plugins/.stats.next/plugin.ini"));
    assert!(storage.exists("plugins/.stats.next/code.ppn"));
    let header =
        paperpaw_sdk::native::read_header(storage, "plugins/.stats.next/code.ppn").unwrap();
    assert_eq!(
        paperpaw_sdk::native::fetched_code(storage, "stats"),
        Some(header.sha256)
    );
    // Not as the code installed now.
    assert!(paperpaw_sdk::native::installed_code(storage, "stats").is_none());
    let shown = device.shown();
    assert!(shown.contains("Install"), "{shown}");
    assert!(!shown.contains("cannot come along"), "{shown}");
}
