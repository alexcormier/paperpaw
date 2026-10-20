//! The store with the real kernel on an X4: the list is fetched for the
//! device and shown by group, a plugin's page fetched, the plugin
//! installed file by file, checked and loaded at once, updated to a newer
//! version and removed; a damaged download is not installed, answers to a
//! download stopped are passed over, and a search finds by name and
//! summary.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_store::{OPEN, Store};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
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

const LIST: &str = "https://paperpaw.org/api/v1/plugins.json?device=x4&lang=en";
const PAGE: &str = "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en";

fn manifest(version: &str) -> String {
    format!(
        "[plugin]\nid = airy\nname = Airy\ndescription = Roomy.\nversion = {version}\nformats = 1\n"
    )
}

const CATALOG_DE: &str =
    "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\nairy.hello = Hallo\n";

/// The list for the X4: Airy in `version`, then a plugin of another
/// group, and one with code the device passes over.
fn list(version: &str) -> String {
    format!(
        r#"{{"format": 2, "plugins": [
  {{"id": "airy", "name": "Airy", "summary": "Light headers and wide margins.",
    "group": "reading", "author": "Someone", "licence": "MIT", "kind": "data",
    "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": 300}}}},
  {{"id": "night-keys", "name": "Night keys", "summary": "Keys for reading in the dark.",
    "group": "system", "author": "Someone", "licence": "MIT", "kind": "data",
    "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 100}}}},
  {{"id": "reader", "name": "Reader", "summary": "Reads books.", "group": "reading",
    "kind": "code", "latest": {{"version": "0.4.0"}}}}
]}}"#
    )
}

/// Airy's page at the library, its files relative to the page.
fn page(version: &str, manifest: &str, catalog: &str) -> String {
    let file = |path: &str, data: &str| {
        format!(
            r#"{{"path": "{path}", "url": "airy/{version}/{path}", "size": {}, "sha1": "{}"}}"#,
            data.len(),
            paperpaw_digest::sha1::hex(data.as_bytes())
        )
    };
    format!(
        r#"{{"format": 2, "plugin": {{"id": "airy", "name": "Airy",
  "summary": "Light headers and wide margins.", "group": "reading",
  "author": "Someone", "licence": "MIT", "kind": "data",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": 300,
    "files": [{}, {}]}}}}}}"#,
        file("plugin.ini", manifest),
        file("i18n/de.ini", catalog),
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
    /// The store opened and its list answered with `list`.
    fn new(list: &str) -> Self {
        Self::on(Some("x4"), list)
    }

    /// The store opened on a device libraries call `device`.
    fn on(device: Option<&'static str>, list: &str) -> Self {
        let storage = Shared::default();
        let mut builder = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_fonts_browser::FontsBrowser)
            .plugin(Store);
        if let Some(device) = device {
            builder = builder.catalog_device(device);
        }
        let mut kernel = builder.build().unwrap();
        kernel.start(Instant(0));
        let mut shopping = Self {
            kernel,
            storage,
            now: 0,
        };
        shopping.run(OPEN);
        if device.is_some() {
            let call = shopping.request(LIST);
            shopping.answer(&call, 200, list.as_bytes());
        }
        shopping
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The requests the kernel asks the shell to make now.
    fn requests(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    /// The one request for `url` asked for now.
    fn request(&mut self, url: &str) -> HttpCall {
        let mut calls: Vec<HttpCall> = self
            .requests()
            .into_iter()
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

    /// Airy's page opened from the list (the entry after *Search*), its
    /// page at the library answered with `page`.
    fn open_airy(&mut self, page: &str) {
        self.run(actions::DOWN);
        self.run(actions::CONFIRM);
        let call = self.request(PAGE);
        self.answer(&call, 200, page.as_bytes());
    }

    /// The first row of Airy's page chosen (*Install* or *Update*), its
    /// files answered with `manifest` and `catalog`.
    fn install(&mut self, version: &str, manifest: &str, catalog: &str) {
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
    let mut shopping = Shopping::new(&list("1.0.0"));
    let shown = shopping.shown();
    assert!(shown.contains("More plugins"), "{shown}");
    // Search first, then the groups in the list's order, the fonts with
    // the looks.
    let at = |text: &str| {
        shown
            .find(text)
            .unwrap_or_else(|| panic!("{text}: {shown}"))
    };
    assert!(at("Search") < at("Reading"), "{shown}");
    assert!(at("Reading") < at("Airy"), "{shown}");
    assert!(at("Airy") < at("Look"), "{shown}");
    assert!(at("Look") < at("Fonts"), "{shown}");
    assert!(shown.contains("Light headers and wide margins."), "{shown}");
    // The next page: the system, its heading again.
    shopping.run(actions::NEXT_PAGE);
    let shown = shopping.shown();
    assert!(shown.contains("System | Night keys"), "{shown}");
    // With code: not for the store.
    assert!(!shown.contains("Reads books."), "{shown}");
    // Kept for the next visit, the list itself gone.
    let kept = ".paperpaw/plugins/store";
    assert!(shopping.storage.exists(&format!("{kept}/index.txt")));
    assert!(!shopping.storage.exists(&format!("{kept}/list.part")));
}

#[test]
fn a_plugin_is_installed_updated_and_removed() {
    let old = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0"));
    shopping.open_airy(&page("1.0.0", &old, CATALOG_DE));
    let shown = shopping.shown();
    assert!(shown.contains("By Someone · Licence: MIT"), "{shown}");
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

    // A newer version: offered, and it takes the old one's place, in the
    // same folder.
    let new = manifest("1.1.0");
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    let call = shopping.request(LIST);
    shopping.answer(&call, 200, list("1.1.0").as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Update"), "{shown}");
    shopping.open_airy(&page("1.1.0", &new, CATALOG_DE));
    shopping.install("1.1.0", &new, CATALOG_DE);
    assert_eq!(shopping.installed(), Some(Version::new(1, 1, 0)));
    assert!(shopping.storage.exists("plugins/airy/plugin.ini"));
    assert!(!shopping.storage.exists("plugins/airy-2/plugin.ini"));

    // *Remove*: asked first.
    let shown = shopping.shown();
    assert!(!shown.contains("Update"), "{shown}");
    shopping.run(actions::CONFIRM);
    let shown = shopping.shown();
    assert!(shown.contains("Remove Airy"), "{shown}");
    shopping.run(actions::CONFIRM);
    assert_eq!(shopping.installed(), None);
    assert!(!shopping.storage.exists("plugins/airy/plugin.ini"));
    let shown = shopping.shown();
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn a_damaged_download_is_not_installed() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0"));
    shopping.open_airy(&page("1.0.0", &manifest, CATALOG_DE));
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
fn answers_to_a_download_stopped_are_passed_over() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0"));
    shopping.open_airy(&page("1.0.0", &manifest, CATALOG_DE));
    let first = "https://paperpaw.org/api/v1/plugins/airy/1.0.0/plugin.ini";
    // *Install*, *Stop downloading*, *Install* again.
    shopping.run(actions::CONFIRM);
    let stopped = shopping.request(first);
    shopping.run(actions::CONFIRM);
    shopping.run(actions::CONFIRM);
    let again = shopping.request(first);
    // The answer for the job stopped comes late: the new job still waits
    // for its own.
    shopping.answer(&stopped, 200, manifest.as_bytes());
    assert!(shopping.requests().is_empty());
    shopping.answer(&again, 200, manifest.as_bytes());
    shopping.request("https://paperpaw.org/api/v1/plugins/airy/1.0.0/i18n/de.ini");
}

#[test]
fn a_search_finds_names_and_summaries() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    // *Search*, answered as the keyboard answers.
    shopping.run(actions::CONFIRM);
    shopping.now += 100;
    let typed = Params::new().with(params::TEXT, "Dark");
    shopping.kernel.send_event(
        Instant(shopping.now),
        Id::new_static("store.searched"),
        typed,
    );
    let shown = shopping.shown();
    assert!(shown.contains("Search: Dark"), "{shown}");
    assert!(shown.contains("Night keys"), "{shown}");
    assert!(!shown.contains("Airy"), "{shown}");
}

#[test]
fn a_device_no_library_knows_is_told_so() {
    let mut shopping = Shopping::on(None, "");
    assert!(shopping.requests().is_empty());
    let shown = shopping.shown();
    assert!(shown.contains("No plugins for this device"), "{shown}");
}

#[test]
fn fonts_open_the_fonts_browser() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    // Down from Search past Airy to Fonts.
    shopping.run(actions::DOWN);
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    shopping.request("https://paperpaw.org/api/v1/fonts.json");
}
