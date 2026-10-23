//! Code plugins in the emulator (ADR 0024): a plugin's crate built for
//! this computer (`cargo build --release`, a `cdylib` with `plugin!`) is
//! `code-host.dll`, `code-host.so` or `code-host.dylib` in its folder of
//! the emulated card (`plugins/<id>/`), beside its `plugin.ini`; the emulator loads it as
//! Android loads `code.so`, with the same entry and table as `.ppn` files
//! on the X4. It must be built by the same compiler, for this computer,
//! against the same SDK as this emulator ([`FINGERPRINT`], which
//! `--stdio` says), and installed: only a file whose SHA-256 the card
//! has (`native::remember_code`) runs, as on the devices, which
//! [`trust`] gives every one on a card (`--trust-code`).
//!
//! Each file is copied before it is loaded, so a new build can replace
//! it while the emulator runs; it is loaded anew at the next start (a
//! restart, `reload` over `--stdio`).

use std::alloc::Layout;
use std::sync::OnceLock;
use std::time::Instant as Clock;

use paperpaw_core::sdk::native::support::{Api, ApiSlot, Entry};
use paperpaw_core::sdk::native::{FINGERPRINT, Native, take};
use paperpaw_core::sdk::{Plugin, PluginProblem};
use paperpaw_core::{CodeFile, CodeLoader};
use paperpaw_platform::Storage;

/// The name of a code plugin's file built for this computer.
pub fn file_name() -> String {
    format!("code-host{}", std::env::consts::DLL_SUFFIX)
}

/// How the emulator loads code plugins: files named [`file_name`], fit
/// for this build.
pub fn native() -> Native {
    Native::new(file_name())
}

/// The largest code plugin file read.
const MAX_FILE: usize = 256 << 20;

/// The folder this process copies code plugins into before it loads
/// them: its own, made at the first ([`crate::private::dir`]).
fn copies() -> std::io::Result<&'static std::path::Path> {
    static COPIES: OnceLock<std::path::PathBuf> = OnceLock::new();
    if let Some(dir) = COPIES.get() {
        return Ok(dir);
    }
    let dir = crate::private::dir("paperpaw-code")?;
    Ok(COPIES.get_or_init(|| dir))
}

unsafe extern "C" fn alloc(size: usize, align: usize) -> *mut u8 {
    match Layout::from_size_align(size, align) {
        // SAFETY: not zero-sized, as `GlobalAlloc::alloc`.
        Ok(layout) if size > 0 => unsafe { std::alloc::alloc(layout) },
        _ => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn dealloc(ptr: *mut u8, size: usize, align: usize) {
    if let Ok(layout) = Layout::from_size_align(size, align) {
        // SAFETY: given by `alloc` or `realloc` with this layout.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

unsafe extern "C" fn realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
    match Layout::from_size_align(size, align) {
        // SAFETY: given by `alloc` or `realloc` with this layout.
        Ok(layout) if new_size > 0 => unsafe { std::alloc::realloc(ptr, layout, new_size) },
        _ => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn panic(message: *const u8, len: usize) -> ! {
    // SAFETY: the plugin's message, `len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(message, len) };
    panic!("a code plugin panicked: {}", String::from_utf8_lossy(bytes));
}

extern "C" fn micros() -> u64 {
    static START: OnceLock<Clock> = OnceLock::new();
    START.get_or_init(Clock::now).elapsed().as_micros() as u64
}

unsafe extern "C" fn icon(name: *const u8, len: usize) -> u32 {
    // SAFETY: the plugin's text, `len` bytes, read while this answers.
    let bytes = unsafe { std::slice::from_raw_parts(name, len) };
    std::str::from_utf8(bytes)
        .ok()
        .and_then(paperpaw_core::sdk::icons::char)
        .map_or(0, u32::from)
}

/// The emulator's table, as a firmware's: the computer's heap (one for
/// both sides), its clock.
static API: Api = Api {
    size: Api::SIZE,
    alloc,
    dealloc,
    realloc,
    panic,
    micros,
    icon,
};

/// Loads code plugins built for this computer.
#[derive(Debug, Default)]
pub struct HostLoader;

impl CodeLoader for HostLoader {
    fn load(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        // Not the device's heap: its code lies in flash (or the PSRAM it
        // runs from), never on the heap a plugin's budget counts.
        crate::budget::untracked(|| load(storage, code))
    }
}

/// [`HostLoader::load`], its memory not counted against the budget.
fn load(storage: &dyn Storage, code: &CodeFile<'_>) -> Result<Box<dyn Plugin>, PluginProblem> {
    let bytes = paperpaw_core::sdk::read_file(storage, code.path, MAX_FILE)
        .map_err(|_| PluginProblem::Damaged)?;
    // Only the file installed.
    if paperpaw_core::sdk::digest::sha256::digest(&bytes) != code.sha256 {
        return Err(PluginProblem::Changed);
    }
    // A copy of its own (the build may change the file meanwhile), in a
    // folder this process made for itself, made anew, then read back and
    // checked again: only what was checked is loaded.
    let copy = copies().map_err(|_| PluginProblem::Damaged)?.join(format!(
        "{}-{}{}",
        code.id.as_str(),
        paperpaw_core::sdk::digest::hex::encode(&code.sha256[..8]),
        std::env::consts::DLL_SUFFIX
    ));
    if !copy.exists() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&copy)
            .map_err(|_| PluginProblem::Damaged)?;
        std::io::Write::write_all(&mut file, &bytes).map_err(|_| PluginProblem::Damaged)?;
    }
    let copied = std::fs::read(&copy).map_err(|_| PluginProblem::Damaged)?;
    if paperpaw_core::sdk::digest::sha256::digest(&copied) != code.sha256 {
        return Err(PluginProblem::Changed);
    }
    // SAFETY: a file the reader installed on the card (`trust`), its
    // SHA-256 the one installed; a code plugin runs as part of
    // PaperPaw, without a sandbox (ADR 0024).
    let library = unsafe { libloading::Library::new(&copy) }.map_err(|_| PluginProblem::Damaged)?;
    // SAFETY: the statics `plugin!` exports, of these types.
    let (slot, entry) = unsafe {
        let slot = library
            .get::<*const ApiSlot>(b"PAPERPAW_API\0")
            .map_err(|_| PluginProblem::Damaged)?;
        let entry = library
            .get::<*const Entry>(b"PAPERPAW_ENTRY\0")
            .map_err(|_| PluginProblem::Damaged)?;
        (&**slot, &**entry)
    };
    if entry.fingerprint != FINGERPRINT.0 {
        return Err(PluginProblem::OtherBuild);
    }
    slot.set(&API);
    // SAFETY: a plugin of this build's fingerprint, made once.
    let plugin = unsafe { take((entry.make)()) }.ok_or(PluginProblem::Damaged)?;
    if plugin.manifest().id != *code.id {
        return Err(PluginProblem::Mismatch);
    }
    // Its code stays loaded as long as the emulator runs: what it made
    // is in use.
    std::mem::forget(library);
    Ok(plugin)
}

/// Installs every code plugin built for this computer on the card in
/// `root` (`plugins/<id>/code-host.*`): the reader's own
/// builds, as an author names a file over USB on the devices. Returns
/// those installed.
pub fn trust(root: &std::path::Path) -> std::io::Result<Vec<String>> {
    let storage = crate::device::library(root)?;
    let plugins = root.join(paperpaw_core::sdk::builtin::PLUGIN_DIR);
    let mut installed = Vec::new();
    let Ok(folders) = std::fs::read_dir(&plugins) else {
        return Ok(installed);
    };
    for folder in folders.flatten() {
        let file = folder.path().join(file_name());
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        let id = folder.file_name().to_string_lossy().into_owned();
        let sha256 = paperpaw_core::sdk::digest::sha256::digest(&bytes);
        if paperpaw_core::sdk::native::remember_code(&storage, &id, &sha256) {
            installed.push(id);
        }
    }
    installed.sort();
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A library that has a code plugin installed runs none of it unless
    /// the one who starts the emulator says so (`--trust-code`): a
    /// library copied from elsewhere would otherwise run its code.
    #[test]
    fn code_runs_only_where_the_caller_trusts_it() {
        let _one = crate::one_device_at_a_time();
        let root = crate::private::dir("paperpaw-trust-test").unwrap();
        let hello = root.join("plugins/hello");
        std::fs::create_dir_all(&hello).unwrap();
        std::fs::write(
            hello.join("plugin.ini"),
            "[plugin]
id = hello
name = Hello
version = 0.1.0
",
        )
        .unwrap();
        std::fs::write(hello.join(file_name()), b"not a library").unwrap();
        // Installed, as by `--trust-code` before.
        assert_eq!(trust(&root).unwrap(), ["hello"]);
        let problem = |trusted: bool| {
            crate::device::set_code_plugins(trusted);
            let storage = crate::device::library(&root).unwrap();
            let profile = crate::profiles::find("boox-go-7").unwrap();
            let device = crate::device::Device::new(profile, None, storage, true).unwrap();
            device.kernel.plugins().get("hello").and_then(|p| p.problem)
        };
        assert_eq!(
            problem(false),
            Some(PluginProblem::Unsupported),
            "not loaded"
        );
        assert_eq!(
            problem(true),
            Some(PluginProblem::Damaged),
            "loaded, and found no library"
        );
        crate::device::set_code_plugins(false);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A code plugin's file installed from the card: named as installed
    /// with its SHA-256; a folder without one is passed over.
    #[test]
    fn the_readers_own_builds_are_installed() {
        let root = std::env::temp_dir().join(format!("paperpaw-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hello = root.join("plugins/hello");
        std::fs::create_dir_all(&hello).unwrap();
        std::fs::create_dir_all(root.join("plugins/data-only")).unwrap();
        std::fs::write(hello.join(file_name()), b"not a library").unwrap();
        assert_eq!(trust(&root).unwrap(), ["hello"]);
        let storage = crate::device::library(&root).unwrap();
        assert_eq!(
            paperpaw_core::sdk::native::installed_code(&storage, "hello"),
            Some(paperpaw_core::sdk::digest::sha256::digest(b"not a library"))
        );
        // Installed but no library: said, not fatal.
        let id = paperpaw_core::sdk::Id::new_static("hello");
        let path = format!("plugins/hello/{}", file_name());
        let code = CodeFile {
            id: &id,
            path: &path,
            sha256: paperpaw_core::sdk::digest::sha256::digest(b"not a library"),
        };
        assert_eq!(
            HostLoader.load(&storage, &code).err(),
            Some(PluginProblem::Damaged)
        );
        let changed = CodeFile {
            sha256: [0; 32],
            ..code
        };
        assert_eq!(
            HostLoader.load(&storage, &changed).err(),
            Some(PluginProblem::Changed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
