//! One emulated device in the browser: the desktop emulator's device model
//! (kernel, panel, profile, heap budget) with its files in memory, the
//! sample books and the data plugins given to it, as a card would have
//! them. Nothing here knows the browser: the exports ([`crate::exports`])
//! pass its calls on, and the tests drive it directly.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use paperpaw_core::sdk::install::{is_plugin_name, is_safe_path};
use paperpaw_core::sdk::{MemoryStorage, Storage};
use paperpaw_dev::report::json_string;
use paperpaw_dev::target::DataPlugin;
use paperpaw_emulator::device::Device;
use paperpaw_emulator::profiles::{self, Profile};
use paperpaw_emulator::{budget, trace};
use paperpaw_platform::{
    ButtonState, InputEvent, PixelFormat, Point, PointerEvent, PointerPhase, PointerTool,
};

/// The books every library starts with (from `samples/library`).
const BOOKS: &[(&str, &[u8])] = &[
    (
        "books/alice-in-wonderland.epub",
        include_bytes!("../../../samples/library/books/alice-in-wonderland.epub"),
    ),
    (
        "books/die-verwandlung.epub",
        include_bytes!("../../../samples/library/books/die-verwandlung.epub"),
    ),
];

/// The devices the browser offers, as `paperpaw-dev`'s galleries show
/// them.
pub const PROFILES: [&str; 3] = ["xteink-x4", "xteink-x4-pro", "boox-go-7"];

/// PaperPaw's version, and the commit the module was built from where
/// the build said (`PAPERPAW_COMMIT`, set by `build.py`).
pub fn version() -> String {
    match option_env!("PAPERPAW_COMMIT") {
        Some(commit) if !commit.is_empty() => format!("{} ({commit})", env!("CARGO_PKG_VERSION")),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Wake-ups handled in one call of [`Emulator::advance`], at most.
const MAX_TICKS: usize = 100;

/// A device running the standard app with the data plugins given.
#[derive(Debug)]
pub struct Emulator {
    profile: &'static Profile,
    language: String,
    dark: bool,
    /// The device's storage: the books, the data plugins.
    files: Rc<MemoryStorage>,
    /// The ids of the data plugins given (their files are on the card
    /// alone).
    plugins: BTreeSet<String>,
    device: Option<Device>,
    /// What the panel shows, four bytes a pixel (RGBA), for a canvas.
    frame: Vec<u8>,
}

impl Emulator {
    /// A device of `profile` (one of [`PROFILES`], or any profile of the
    /// emulator), its texts in `language`, in night mode if `dark`. Not
    /// started yet: data plugins are given first ([`Emulator::put_file`]).
    pub fn new(profile: &str, language: &str, dark: bool) -> Result<Self, String> {
        let profile =
            profiles::find(profile).ok_or_else(|| format!("no device profile `{profile}`"))?;
        if language.is_empty()
            || !language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(format!("no language `{language}`"));
        }
        let files = MemoryStorage::new();
        for (path, bytes) in BOOKS {
            files
                .write(path, bytes)
                .map_err(|e| format!("{path}: {e:?}"))?;
        }
        // The first-run setup counts as done.
        files
            .write(".paperpaw/plugins/setup/done", b"")
            .map_err(|e| format!("setup: {e:?}"))?;
        Ok(Self {
            profile,
            language: language.to_string(),
            dark,
            files: Rc::new(files),
            plugins: BTreeSet::new(),
            device: None,
            frame: Vec::new(),
        })
    }

    pub fn profile(&self) -> &'static Profile {
        self.profile
    }

    /// Puts a file of the data plugin `plugin` on the card, as
    /// `plugins/<plugin>/<path>`; seen by the app at the next start
    /// ([`Emulator::restart`]).
    pub fn put_file(&mut self, plugin: &str, path: &str, bytes: &[u8]) -> Result<(), String> {
        if !is_plugin_name(plugin) {
            return Err(format!("`{plugin}` is no plugin's name"));
        }
        if !is_safe_path(path) {
            return Err(format!("`{path}` is no path inside a plugin"));
        }
        let at = format!("plugins/{plugin}/{path}");
        self.files
            .write(&at, bytes)
            .map_err(|e| format!("{at}: {e:?}"))?;
        self.plugins.insert(plugin.to_string());
        Ok(())
    }

    /// Takes the data plugin `plugin` off the card, every file of it
    /// (also those a page put there some other way).
    pub fn remove_plugin(&mut self, plugin: &str) {
        self.plugins.remove(plugin);
        if is_plugin_name(plugin) {
            let _ = self.files.remove_dir_all(&format!("plugins/{plugin}"));
        }
    }

    /// Starts the device: the standard app with the data plugins on the
    /// card. Started already, it starts again (see [`Emulator::restart`]).
    pub fn start(&mut self) -> Result<(), String> {
        if self.device.is_some() {
            self.restart();
            return Ok(());
        }
        // Dark as asked; else as the defaults have it (a theme's too).
        let night = if self.dark { "night-mode = on\n" } else { "" };
        // Started as the first-run setup left it, and without the tips on
        // gestures a new reader sees once.
        let config = format!(
            "[core]\nlanguage = {}\n{night}[home]\ngesture-tips = false\n",
            self.language
        );
        let mut device =
            Device::in_memory(self.profile, Some(config), self.files.clone(), true, None)
                .map_err(|e| e.to_string())?;
        device.start();
        self.device = Some(device);
        Ok(())
    }

    /// Starts the device again, as after a reboot: what changed on the
    /// card (a data plugin's files) is read anew. A few milliseconds.
    pub fn restart(&mut self) {
        if let Some(device) = &mut self.device {
            device.restart();
        }
    }

    /// Whether the device has a button `name`.
    pub fn has_button(&self, name: &str) -> bool {
        self.profile.buttons.contains(&name)
    }

    /// A button pressed or let go; `false` if the device has no such
    /// button.
    pub fn button(&mut self, name: &str, pressed: bool) -> bool {
        let Some(&name) = self.profile.buttons.iter().find(|b| **b == name) else {
            return false;
        };
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        if let Some(device) = &mut self.device {
            device.input(InputEvent::Button { name, state });
        }
        true
    }

    /// A finger put down, moved or lifted at `(x, y)` of the screen;
    /// nothing where the device has no touch screen.
    pub fn pointer(&mut self, phase: PointerPhase, x: i32, y: i32) {
        if !self.profile.device.input.touch {
            return;
        }
        if let Some(device) = &mut self.device {
            device.input(InputEvent::Pointer(PointerEvent {
                id: 0,
                phase,
                point: Point::new(x, y),
                tool: PointerTool::Finger,
                pressure: 1000,
            }));
        }
    }

    /// Lets time pass to `now_ms` (since the emulator started, as the
    /// browser counts it) and does what is due; how many milliseconds
    /// until something is due next, `None` if nothing is.
    pub fn advance(&mut self, now_ms: u64) -> Option<u64> {
        #[cfg(target_arch = "wasm32")]
        paperpaw_emulator::host::set_micros(now_ms.saturating_mul(1000));
        let device = self.device.as_mut()?;
        let now = device.now().millis();
        device.advance(now_ms.saturating_sub(now));
        for _ in 0..MAX_TICKS {
            device.poll_http(None);
            match device.next_wake() {
                Some(at) if at.millis() <= device.now().millis() => device.tick_if_due(),
                _ => break,
            }
        }
        let now = device.now().millis();
        device.next_wake().map(|at| at.millis().saturating_sub(now))
    }

    /// The screen's size in pixels (turned as the reader turned it).
    pub fn size(&self) -> (u16, u16) {
        match &self.device {
            Some(device) => (device.panel.width(), device.panel.height()),
            None => {
                let display = self.profile.device.display;
                (display.width, display.height)
            }
        }
    }

    /// What the panel shows now, four bytes a pixel (RGBA), row by row:
    /// its grays as the panel shows them, a full refresh flashing.
    pub fn frame(&mut self) -> &[u8] {
        self.frame.clear();
        if let Some(device) = &self.device {
            for pixel in device.panel.compose() {
                self.frame.extend_from_slice(&[
                    (pixel >> 16) as u8,
                    (pixel >> 8) as u8,
                    pixel as u8,
                    255,
                ]);
            }
        }
        &self.frame
    }

    /// What the panel shows once a full refresh has flashed, four bytes a
    /// pixel (RGBA): for screenshots.
    pub fn shown(&self) -> Vec<u8> {
        let Some(device) = &self.device else {
            return Vec::new();
        };
        device
            .panel
            .rgb_bytes()
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    }

    /// Whether the panel still changes by itself (a full refresh
    /// flashing): frames are wanted until it is done.
    pub fn animating(&self) -> bool {
        self.device.as_ref().is_some_and(|d| d.panel.is_animating())
    }

    /// The data plugins given, checked by `paperpaw-dev` on this device's
    /// profile with `commands` (none: package, lint, visual and reach);
    /// its report as JSON (`report.json`).
    pub fn check(&mut self, commands: &[&str]) -> Result<String, String> {
        if self.plugins.is_empty() {
            return Err("no data plugin on the card to check".to_string());
        }
        // Read from the card, held while checked.
        let plugins = self
            .plugins
            .iter()
            .map(|id| DataPlugin::new(id, files_under(&*self.files, &format!("plugins/{id}"))))
            .collect();
        let report = paperpaw_dev::data::check(plugins, vec![self.profile], commands);
        // The checks measure their own devices; this one keeps its budget
        // and records no frames.
        trace::stop_recording();
        budget::end_override(self.profile.heap_budget);
        Ok(report?.json())
    }

    /// The device as the page shows it, as JSON: its name, screen size,
    /// grays and dpi, whether it has touch, its buttons and those in a
    /// row below the screen.
    pub fn device_json(&self) -> String {
        let device = &self.profile.device;
        let (width, height) = self.size();
        let grays = match device.display.format {
            PixelFormat::Mono1 => 2,
            PixelFormat::Gray2 => 4,
            PixelFormat::Gray4 => 16,
            _ => 256,
        };
        let list = |names: &[&str]| {
            names
                .iter()
                .map(|n| json_string(n))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "{{\"id\":{},\"name\":{},\"width\":{width},\"height\":{height},\"dpi\":{},\"grays\":{grays},\"color\":{},\"touch\":{},\"buttons\":[{}],\"row\":[{}],\"version\":{}}}",
            json_string(device.id),
            json_string(device.name),
            device.display.dpi,
            device.display.color.is_some(),
            device.input.touch,
            list(self.profile.buttons),
            list(self.profile.button_row),
            json_string(&version()),
        )
    }
}

/// Every file under `dir` of `storage`, by its path from `dir`.
fn files_under(storage: &dyn Storage, dir: &str) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut folders = vec![String::new()];
    while let Some(folder) = folders.pop() {
        let at = if folder.is_empty() {
            dir.to_string()
        } else {
            format!("{dir}/{folder}")
        };
        let mut entries = Vec::new();
        let _ = storage.list(&at, &mut |entry| {
            entries.push((entry.name.to_string(), entry.is_dir));
        });
        for (name, is_dir) in entries {
            let path = if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            if is_dir {
                folders.push(path);
            } else if let Ok(bytes) =
                paperpaw_core::sdk::read_file(storage, &format!("{dir}/{path}"), usize::MAX)
            {
                out.insert(path, bytes);
            }
        }
    }
    out
}
