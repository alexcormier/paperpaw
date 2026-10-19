//! The store with the real kernel on an X4: the list is fetched and shown
//! by group, a plugin is installed file by file, checked and loaded at
//! once, updated to a newer version and removed; a damaged download is
//! not installed.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_store::{OPEN, Store};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Storage, StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "xteink-x4",
    name: "Xteink X4",
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

const LIST: &str = "https://paperpaw.org/api/v1/plugins.json";

fn manifest(version: &str) -> String {
    format!(
        "[plugin]\nid = airy\nname = Airy\ndescription = Roomy.\nversion = {version}\nformats = 1\n"
    )
}

const CATALOG_DE: &str =
    "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\nairy.hello = Hallo\n";

/// The list: Airy in `version` for the X4 (its files relative to the
/// list), and plugins that do not fit.
fn list(version: &str, manifest: &str, catalog: &str) -> String {
    let file = |path: &str, data: &str| {
        format!(
            r#"{{"path": "{path}", "url": "/api/v1/plugins/airy/{version}/{path}", "size": {}, "sha1": "{}"}}"#,
            data.len(),
            paperpaw_digest::sha1::hex(data.as_bytes())
        )
    };
    format!(
        r#"{{"format": 1, "plugins": [
  {{"id": "airy", "name": "Airy", "summary": "Light headers and wide margins.",
    "i18n": {{"de": {{"name": "Luftig"}}}},
    "kind": "data", "group": "reading", "author": "Someone", "licence": "MIT",
    "devices": ["x4", "x4-pro"], "built_in": false,
    "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1,
      "files": [{}, {}]}}}},
  {{"id": "phone-only", "name": "Phone only", "kind": "data", "group": "look",
    "devices": ["android"],
    "latest": {{"version": "1.0.0", "files": [{}]}}}},
  {{"id": "reader", "name": "Reader", "kind": "code", "built_in": true, "group": "reading",
    "latest": {{"version": "0.4.0"}}}}
]}}"#,
        file("plugin.ini", manifest),
        file("i18n/de.ini", catalog),
        file("plugin.ini", manifest),
    )
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
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
}

struct Shopping {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Shopping {
    /// The store opened and its list answered.
    fn new(list: &str) -> Self {
        let storage = Shared::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_fonts_browser::FontsBrowser)
            .plugin(Store)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut shopping = Self {
            kernel,
            storage,
            now: 0,
        };
        shopping.open(list);
        shopping
    }

    /// The store opened anew, its list answered with `list`.
    fn open(&mut self, list: &str) {
        self.run(OPEN);
        let call = self.request(LIST);
        self.answer(&call, 200, list.as_bytes());
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The one request for `url` the kernel asks the shell to make now.
    fn request(&mut self, url: &str) -> HttpCall {
        let mut calls: Vec<HttpCall> = self
            .kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .filter(|c| c.url == url)
            .collect();
        assert_eq!(calls.len(), 1, "{url}: {calls:?}");
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

    /// The labels of what the screen shows, in order.
    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Airy's page opened from the list (its first entry) and its first
    /// row chosen: *Install* or *Update*; its files answered with
    /// `manifest` and `catalog`.
    fn install(&mut self, version: &str, manifest: &str, catalog: &str) {
        self.run(actions::CONFIRM);
        let shown = self.shown();
        assert!(shown.contains("By Someone · Licence: MIT"), "{shown}");
        self.run(actions::CONFIRM);
        let base = format!("https://paperpaw.org/api/v1/plugins/airy/{version}");
        let call = self.request(&format!("{base}/plugin.ini"));
        assert_eq!(
            call.save_to.as_deref(),
            Some("plugins/.airy.part/plugin.ini")
        );
        self.answer(&call, 200, manifest.as_bytes());
        let shown = self.shown();
        assert!(shown.contains("Stop downloading"), "{shown}");
        let call = self.request(&format!("{base}/i18n/de.ini"));
        self.answer(&call, 200, catalog.as_bytes());
    }

    fn installed(&self) -> Option<Version> {
        self.kernel
            .plugins()
            .get("airy")
            .map(|p| p.manifest.version)
    }
}

#[test]
fn the_list_shows_what_fits_by_group() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0", &manifest, CATALOG_DE));
    let shown = shopping.shown();
    assert!(shown.contains("More plugins"), "{shown}");
    // Its group, then the looks with the fonts.
    let reading = shown.find("Reading").unwrap();
    let airy = shown.find("Airy").unwrap();
    let look = shown.find("Look").unwrap();
    let fonts = shown.find("Fonts").unwrap();
    assert!(reading < airy && airy < look && look < fonts, "{shown}");
    assert!(shown.contains("Light headers and wide margins."), "{shown}");
    // Not for the X4, built in.
    assert!(!shown.contains("Phone only"), "{shown}");
    assert!(!shown.contains("Reader"), "{shown}");
    // Kept for the next visit.
    assert!(
        shopping
            .storage
            .exists(".paperpaw/plugins/store/plugins.json")
    );
}

#[test]
fn a_plugin_is_installed_updated_and_removed() {
    let old = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0", &old, CATALOG_DE));
    shopping.install("1.0.0", &old, CATALOG_DE);
    // Loaded at once, as a plugin of its own.
    assert_eq!(shopping.installed(), Some(Version::new(1, 0, 0)));
    assert!(shopping.storage.exists("plugins/airy/i18n/de.ini"));
    assert!(!shopping.storage.exists("plugins/.airy.part/plugin.ini"));
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/install.txt")
    );
    let shown = shopping.shown();
    assert!(shown.contains("Installed: version 1.0.0"), "{shown}");
    assert!(shown.contains("Remove"), "{shown}");

    // A newer version in the list: offered, and it takes the old one's
    // place.
    let new = manifest("1.1.0");
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.open(&list("1.1.0", &new, CATALOG_DE));
    let shown = shopping.shown();
    assert!(shown.contains("Update"), "{shown}");
    shopping.install("1.1.0", &new, CATALOG_DE);
    assert_eq!(shopping.installed(), Some(Version::new(1, 1, 0)));
    assert!(!shopping.storage.exists("plugins/airy/plugin.ini"));
    assert!(shopping.storage.exists("plugins/airy-2/plugin.ini"));

    // *Remove*: asked first.
    let shown = shopping.shown();
    assert!(!shown.contains("Update"), "{shown}");
    shopping.run(actions::CONFIRM);
    let shown = shopping.shown();
    assert!(shown.contains("Remove Airy"), "{shown}");
    shopping.run(actions::CONFIRM);
    assert_eq!(shopping.installed(), None);
    assert!(!shopping.storage.exists("plugins/airy-2/plugin.ini"));
    let shown = shopping.shown();
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn a_damaged_download_is_not_installed() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0", &manifest, CATALOG_DE));
    shopping.install("1.0.0", &manifest, "[meta]\nlanguage = de\n");
    assert_eq!(shopping.installed(), None);
    assert!(!shopping.storage.exists("plugins/.airy.part/plugin.ini"));
    assert!(!shopping.storage.exists("plugins/airy/plugin.ini"));
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/install.txt")
    );
    // Offered again.
    let shown = shopping.shown();
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn fonts_open_the_fonts_browser() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0", &manifest, CATALOG_DE));
    // Down from Airy to Fonts.
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    shopping.request("https://paperpaw.org/api/v1/fonts.json");
}
