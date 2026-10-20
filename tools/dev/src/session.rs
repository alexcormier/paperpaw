//! One emulated device in a variant, for the checks to drive: a fresh
//! library (the sample books, the data plugins looked at, the first-run
//! setup done), the standard app with the plugins added, simulated time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_core::sdk::semantics::{Described, Part};
use paperpaw_core::sdk::{Id, Params};
use paperpaw_emulator::device::Device;
use paperpaw_emulator::{budget, headless, trace};
use paperpaw_platform::Instant;

use crate::cli::Context;
use crate::image::Image;
use crate::variant::{self, Variant};

/// The books every library starts with (from `samples/library`).
const BOOKS: &[(&str, &[u8])] = &[
    (
        "books/alice-in-wonderland.txt",
        include_bytes!("../../../samples/library/books/alice-in-wonderland.txt"),
    ),
    (
        "books/alice-in-wonderland.epub",
        include_bytes!("../../../samples/library/books/alice-in-wonderland.epub"),
    ),
    (
        "books/die-verwandlung.epub",
        include_bytes!("../../../samples/library/books/die-verwandlung.epub"),
    ),
];

/// A book to open in sessions that read.
pub const BOOK: &str = "books/alice-in-wonderland.epub";

/// Numbers the libraries of one run.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// How a session starts.
#[derive(Debug, Clone, Default)]
pub struct Start {
    /// Settings besides the variant's, as `(id, value)`.
    pub settings: Vec<(String, String)>,
    /// Leave the first-run setup to show (by default it is done).
    pub setup: bool,
    /// Files to put into the library besides the books, as `(path, bytes)`.
    pub files: Vec<(String, Vec<u8>)>,
}

/// A device running the app in a variant.
#[derive(Debug)]
pub struct Session {
    pub device: Device,
    pub variant: Variant,
    root: PathBuf,
    keep: bool,
}

impl Session {
    /// A session as most checks want it: the variant, nothing else.
    pub fn new(context: &Context, variant: &Variant) -> Result<Session, String> {
        Self::start(context, variant, &Start::default())
    }

    pub fn start(context: &Context, variant: &Variant, start: &Start) -> Result<Session, String> {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = context
            .out
            .join("work")
            .join(format!("{n:04}-{}", variant.slug()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |path: &str, bytes: &[u8]| -> Result<(), String> {
            let file = root.join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))
        };
        for (path, bytes) in BOOKS {
            write(path, bytes)?;
        }
        if !start.setup {
            write(".paperpaw/plugins/setup/done", b"")?;
        }
        for (path, bytes) in &start.files {
            write(path, bytes)?;
        }
        if variant.language == crate::lint::PSEUDO {
            for (path, bytes) in crate::lint::pseudo_plugin(context)? {
                write(&path, &bytes)?;
            }
        }
        for plugin in &context.target.data {
            copy_dir(&plugin.folder, &root.join("plugins").join(&plugin.id))?;
        }
        let mut settings: Vec<(&str, &str)> = variant.settings();
        settings.extend(start.settings.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let config = variant::config(settings);
        let storage = paperpaw_fs::DirStorage::new(&root)
            .map_err(|e| format!("library {}: {e}", root.display()))?;
        // The budget is compared after (see `perf`), rather than making
        // the run fail where the device would run out; frames are kept
        // for the checks that look at them.
        budget::override_limit(None);
        crate::probe::set_target(context.target.add);
        trace::record();
        drop(trace::take_frames());
        let mut device = Device::with_plugins(
            variant.profile,
            Some(config),
            storage,
            true,
            Some(crate::probe::add),
        )
        .map_err(|e| format!("{}: {e}", variant.name()))?;
        device.start();
        device.tick_if_due();
        let mut session = Session {
            device,
            variant: variant.clone(),
            root,
            keep: context.keep,
        };
        session.settle();
        Ok(session)
    }

    /// The folder emulating the device's storage.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Lets what is due happen, up to a few seconds of emulated time
    /// (screens that open in steps, books being prepared).
    pub fn settle(&mut self) {
        let end = self.device.now().add_millis(3000);
        for _ in 0..1000 {
            let Some(at) = self.device.next_wake() else {
                return;
            };
            if at > end {
                return;
            }
            let now = self.device.now();
            self.device.advance(at.since(now).max(1));
            self.device.tick_if_due();
        }
    }

    /// Runs script steps as `paperpaw-emu --script` does (see
    /// `shells/desktop/src/headless.rs`).
    pub fn script(&mut self, steps: &str) -> Result<(), String> {
        headless::run_script(&mut self.device, steps)?;
        self.settle();
        Ok(())
    }

    /// Opens a screen with parameters, as a plugin would.
    pub fn open(&mut self, screen: &str, params: &[(&str, &str)]) -> Result<(), String> {
        let id = Id::parse(screen).map_err(|e| format!("{screen}: {e}"))?;
        let mut given = Params::new();
        for (key, value) in params {
            given = given.with(key.to_string(), *value);
        }
        self.device.open_screen(id, given);
        self.settle();
        Ok(())
    }

    /// Changes a setting as the settings screen would.
    pub fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        let mut result = Ok(());
        self.device
            .call(|kernel, now| match kernel.set_setting(now, id, value) {
                Ok(effects) => effects,
                Err(e) => {
                    result = Err(format!("{id} = {value}: {e}"));
                    paperpaw_core::Effects::default()
                }
            });
        self.settle();
        result
    }

    /// Runs an action, as a button bound to it would.
    pub fn run(&mut self, action: &str) -> Result<(), String> {
        let id = Id::parse(action).map_err(|e| format!("{action}: {e}"))?;
        self.device.run_action(id);
        self.settle();
        Ok(())
    }

    /// Presses a button of the device (short).
    pub fn press(&mut self, button: &str) -> Result<(), String> {
        self.script(button)
    }

    /// The screen shown.
    pub fn screen(&self) -> Option<String> {
        self.device.kernel.active_screen().map(ToString::to_string)
    }

    /// The screens open, the one shown last.
    pub fn stack(&self) -> Vec<String> {
        self.device
            .kernel
            .screens()
            .map(ToString::to_string)
            .collect()
    }

    /// What the panel shows.
    pub fn image(&self) -> Image {
        Image::from_rgb(
            u32::from(self.device.panel.width()),
            u32::from(self.device.panel.height()),
            self.device.panel.rgb_bytes(),
        )
    }

    /// What the screen shows, part by part.
    pub fn parts(&mut self) -> Vec<Part> {
        self.device.kernel.describe()
    }

    /// Everything the drawing helpers told as they drew, with clips.
    pub fn described(&mut self) -> Vec<Described> {
        self.device.kernel.described()
    }

    /// The frames drawn since the last call (time, storage calls by band).
    /// Recording starts with the first session.
    pub fn frames(&self) -> Vec<trace::FrameRecord> {
        trace::take_frames()
    }

    pub fn now(&self) -> Instant {
        self.device.now()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Copies a folder with everything in it.
pub fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}
