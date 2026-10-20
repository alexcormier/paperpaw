//! What the store's tests share: an X4, a storage the test looks into
//! while the kernel has it, and the kernel with the store, driven as by
//! the buttons and answered as by the shell.

// Not every test uses every part.
#![allow(dead_code)]

use std::rc::Rc;

use paperpaw_core::{CodeLoader, HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_store::{OPEN, Store};
use paperpaw_sdk::native::Native;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Plugin, PluginProblem, Storage, StorageError,
};

pub const DEVICE: DeviceInfo = DeviceInfo {
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

/// The list for the X4.
pub const LIST: &str = "https://paperpaw.org/api/v1/plugins.json?device=x4&lang=en";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
pub struct Shared(pub Rc<MemoryStorage>);

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

/// Loads no code: code plugins installed in a test start after a restart
/// it never makes.
struct NoCode;

impl CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Damaged)
    }
}

/// The kernel with the store, as the reader uses it.
pub struct Shopping {
    pub kernel: Kernel,
    pub storage: Shared,
    pub now: u64,
}

impl Shopping {
    /// The store opened on the X4 and its list answered with `list`.
    pub fn new(list: &str) -> Self {
        Self::on(Some("x4"), None, list)
    }

    /// The store opened on a device libraries call `device`, loading code
    /// plugins as `native` says, its list answered with `list`.
    pub fn on(device: Option<&'static str>, native: Option<Native>, list: &str) -> Self {
        let storage = Shared::default();
        let mut builder = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_fonts_browser::FontsBrowser)
            // Shows the notices.
            .plugin(paperpaw_plugin_menu::Menu)
            .plugin(Store);
        if let Some(device) = device {
            builder = builder.catalog_device(device);
        }
        if let Some(native) = native {
            builder = builder.code_plugins(native, Box::new(NoCode));
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

    pub fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The requests the kernel asks the shell to make now.
    pub fn requests(&mut self) -> Vec<HttpCall> {
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
    pub fn request(&mut self, url: &str) -> HttpCall {
        let all = self.requests();
        let mut calls: Vec<HttpCall> = all.iter().filter(|c| c.url == url).cloned().collect();
        assert_eq!(calls.len(), 1, "{url}: {all:?}");
        calls.remove(0)
    }

    pub fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) {
        self.now += 100;
        let response = HttpResponse {
            id: call.id,
            status,
            body: body.to_vec(),
            ..HttpResponse::default()
        };
        self.kernel.http_response(Instant(self.now), response);
    }

    /// The one request for `url` answered with `body`.
    pub fn serve(&mut self, url: &str, body: &[u8]) {
        let call = self.request(url);
        self.answer(&call, 200, body);
    }

    /// The labels of what the screen shows, in order.
    pub fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Compares what the screen shows, part by part, with
    /// `tests/snapshots/{name}.txt`; written anew with
    /// `PAPERPAW_SNAPSHOTS=update`.
    pub fn check(&mut self, name: &str) {
        self.kernel.render(&mut |_, _| {});
        let mut shown = String::new();
        for part in self.kernel.describe() {
            let focused = if part.focused { " (focused)" } else { "" };
            let label = part.label.replace('\n', " / ");
            shown.push_str(&format!("{:?} \"{label}\"{focused}\n", part.role));
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("{name}.txt"));
        if std::env::var("PAPERPAW_SNAPSHOTS").as_deref() == Ok("update") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &shown).unwrap();
            return;
        }
        let kept = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} missing: PAPERPAW_SNAPSHOTS=update", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            shown, kept,
            "{name} shows something else now (PAPERPAW_SNAPSHOTS=update if meant)"
        );
    }
}
