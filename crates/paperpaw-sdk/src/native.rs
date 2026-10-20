//! Native code plugins (ADR 0024): a plugin built into a file that a
//! device loads at its start, without a new firmware. It runs as part of
//! PaperPaw, with no sandbox, and its code is the same as a plugin built
//! in: one more crate makes the file, with [`plugin!`](crate::plugin).
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//!
//! paperpaw_sdk::plugin!(my_plugin::MyPlugin::default());
//! ```
//!
//! A plugin file fits a PaperPaw only when both were built by the same
//! compiler for the same chip from the same SDK with the same features
//! (until PaperPaw 1.0, the SDK's types cross between them as they are):
//! [`FINGERPRINT`] says which, and a device loads only files with its own
//! ([`Native::fingerprint`]). `docs/plugins.md` says how a plugin is
//! built, packed (`paperpaw-sign pack`), signed and published.
//!
//! A code plugin is a folder like a data plugin's (`plugin.ini`,
//! translations, settings) with its code as [`Native::file`]; the kernel
//! loads the code of those switched on at the start, and switching one
//! takes a restart. Where PaperPaw loads code plugins, [`Env::native`]
//! says with what (the store takes the build that fits), and whether this
//! start went without them ([`SafeMode`]).
//!
//! [`Env::native`]: crate::Env

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::Id;
use paperpaw_platform::Storage;

pub use paperpaw_native::signature::{self, BadSignature};
pub use paperpaw_native::{Chip, HEADER_LEN, Header, Problem, Signature};

/// What this build of the SDK makes plugins fit: the compiler, target,
/// SDK version and features (see the [module documentation](self)).
pub const FINGERPRINT: Fingerprint = {
    let build = match u64::from_str_radix(env!("PAPERPAW_NATIVE_BUILD"), 16) {
        Ok(build) => build,
        Err(_) => panic!("PAPERPAW_NATIVE_BUILD is not hexadecimal"),
    };
    // Features of crates below the SDK that change their types' layouts,
    // whichever crate switched them on.
    let outlines = paperpaw_text::OUTLINES as u64;
    Fingerprint((build ^ outlines).wrapping_mul(0x0100_0000_01b3))
};

/// The compiler and target this build of the SDK was made with, e.g.
/// `rustc 1.99.0 (b940084d7 2026-09-28), riscv32imc-unknown-none-elf`:
/// what a plugin must be built with to fit it.
pub const TOOLCHAIN: &str = env!("PAPERPAW_NATIVE_TOOLCHAIN");

/// The names a plugin folder's code has: `code.ppn` on the X4 and the X4
/// Pro, `code.so` on Android. A folder with one is a code plugin's, also
/// where PaperPaw loads none.
pub const CODE_FILES: [&str; 2] = ["code.ppn", "code.so"];

/// Checks the plugin file at `path` whole, reading it once: its header,
/// its length, its digest and, if it is signed, its author's signature.
/// Its header and its author's signature (`None` if it is not signed);
/// whether it fits this device and build is for the caller to see
/// ([`Header::chip`], [`Header::fingerprint`]), and whether the author is
/// one known (a store remembers the key of the first install).
pub fn check_file(
    storage: &dyn Storage,
    path: &str,
) -> Result<(Header, Option<Signature>), Problem> {
    let mut head = [0u8; HEADER_LEN];
    read_exact(storage, path, 0, &mut head)?;
    let header = Header::read(&head)?;
    let size = storage.size(path).map_err(|_| Problem::Length)?;
    let end = header.file_len();
    let signature = if header.signed(size)? {
        let mut trailer = [0u8; signature::SIGNATURE_LEN];
        read_exact(storage, path, end, &mut trailer)?;
        let signature = Signature::read(&trailer).ok_or(Problem::Signature)?;
        if u64::from(signature.signed_len) != end {
            return Err(Problem::Signature);
        }
        Some(signature)
    } else {
        None
    };
    let mut author = match &signature {
        Some(signature) => Some(signature.check().map_err(|_| Problem::Signature)?),
        None => None,
    };
    if let Some(check) = author.as_mut() {
        check.absorb(&head);
    }
    let mut digest = header.digest();
    let mut buf = [0u8; 512];
    let mut at = HEADER_LEN as u64;
    while at < end {
        let len = (end - at).min(buf.len() as u64) as usize;
        read_exact(storage, path, at, &mut buf[..len])?;
        digest.update(&buf[..len]);
        if let Some(check) = author.as_mut() {
            check.absorb(&buf[..len]);
        }
        at += len as u64;
    }
    if !header.holds(digest) {
        return Err(Problem::Damaged);
    }
    if let Some(check) = author {
        check.finish().map_err(|_| Problem::Signature)?;
    }
    Ok((header, signature))
}

/// Where the keys of the authors of the code plugins installed are kept:
/// one file for every installer (a store, the firmware's first start after
/// the web flasher), so a key stays known whichever installs.
const AUTHORS: &str = crate::data_path!("plugin-authors.txt");

/// Whose a plugin's code is, by its author's key, as this device knows
/// the plugin's author (trust on first use, ADR 0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    /// The key remembered for the plugin.
    Known,
    /// No key is remembered for the plugin yet: its first install.
    New,
    /// Another key than the one remembered: an update of it is installed
    /// only when the reader says so, after a clear warning.
    Other,
}

/// Whether `key` (a [`Signature::key`]) is the author of `plugin` known on
/// this device, the first one, or another.
pub fn author(storage: &dyn Storage, plugin: &str, key: &[u8; 32]) -> Author {
    match known_author(storage, plugin) {
        Some(known) if known == *key => Author::Known,
        Some(_) => Author::Other,
        None => Author::New,
    }
}

/// The key of the author of `plugin`'s code, as remembered at its first
/// install ([`remember_author`]).
pub fn known_author(storage: &dyn Storage, plugin: &str) -> Option<[u8; 32]> {
    entry(storage, AUTHORS, plugin)
}

/// Remembers `key` as the author of `plugin`'s code, in place of one
/// known before: at its first install, and when the reader installed an
/// update by another; `false` if it could not be kept (the file left as
/// it is where it could not be read).
pub fn remember_author(storage: &dyn Storage, plugin: &str, key: &[u8; 32]) -> bool {
    keep_entry(storage, AUTHORS, plugin, key)
}

/// Where the code plugins' files are named as they were installed, by the
/// SHA-256 their headers carry (`Header::sha256`, which holds for all of
/// the file but its signature): beside the authors' keys, where neither
/// the Wi-Fi transfer nor plugins' own folders reach.
const INSTALLED: &str = crate::data_path!("plugin-code.txt");

/// The code of `plugin` as installed ([`remember_code`]): the SHA-256 of
/// its file's header. The kernel loads a code plugin's file only when its
/// header says the same, so that a file put into a plugin's folder
/// otherwise (copied there by hand, or over the Wi-Fi transfer) does not
/// run in its place.
pub fn installed_code(storage: &dyn Storage, plugin: &str) -> Option<[u8; 32]> {
    entry(storage, INSTALLED, plugin)
}

/// Names the code of `plugin` as installed, by its header's SHA-256, in
/// place of the version before: every installer does, once it checked the
/// file ([`check_file`]) and its author; `false` if it could not be kept
/// (the file left as it is where it could not be read).
pub fn remember_code(storage: &dyn Storage, plugin: &str, sha256: &[u8; 32]) -> bool {
    keep_entry(storage, INSTALLED, plugin, sha256)
}

/// The 32 bytes kept for `plugin` in `file`, a line `<plugin>\t<hex>` each.
fn entry(storage: &dyn Storage, file: &str, plugin: &str) -> Option<[u8; 32]> {
    let text = crate::read_file(storage, file, 64 * 1024).ok()?;
    let text = core::str::from_utf8(&text).ok()?;
    let hex = text
        .lines()
        .find_map(|line| line.strip_prefix(plugin)?.strip_prefix('\t'))?;
    paperpaw_digest::hex::decode(hex.trim())?.try_into().ok()
}

/// Keeps `value` for `plugin` in `file`, in place of what was kept. A
/// file that is there but cannot be read (a failing card, too large) is
/// not written over: it holds every other plugin's.
fn keep_entry(storage: &dyn Storage, file: &str, plugin: &str, value: &[u8; 32]) -> bool {
    let Ok(kept) = crate::files::load_text_for_update(storage, file, 64 * 1024) else {
        return false;
    };
    let kept = kept.unwrap_or_default();
    let mut text: String = kept
        .lines()
        .filter(|line| line.split('\t').next() != Some(plugin))
        .map(|line| alloc::format!("{line}\n"))
        .collect();
    text.push_str(&alloc::format!(
        "{plugin}\t{}\n",
        paperpaw_digest::hex::encode(value)
    ));
    let _ = storage.create_dir(crate::data::DIR);
    storage.write(file, text.as_bytes()).is_ok()
}

/// Fills `buf` from `path` at `at`.
fn read_exact(
    storage: &dyn Storage,
    path: &str,
    mut at: u64,
    mut buf: &mut [u8],
) -> Result<(), Problem> {
    while !buf.is_empty() {
        match storage.read_at(path, at, buf) {
            Ok(n) if n > 0 => {
                at += n as u64;
                buf = &mut buf[n..];
            }
            _ => return Err(Problem::Length),
        }
    }
    Ok(())
}

/// Which build a code plugin fits: a hash of what it was built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub u64);

impl Fingerprint {
    /// From 16 hexadecimal digits, as the plugin library lists builds.
    pub fn parse(text: &str) -> Option<Fingerprint> {
        let text = text.trim();
        if text.len() != 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(text, 16).ok().map(Fingerprint)
    }
}

/// 16 lower-case hexadecimal digits, as the plugin library lists builds
/// (`abi`).
impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// How this PaperPaw loads code plugins
/// ([`Env::native`](crate::Env)): the shell tells the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Native {
    /// The fingerprint plugin files must have to be loaded here.
    pub fingerprint: Fingerprint,
    /// What they must be built with ([`TOOLCHAIN`] of the shell's build).
    pub toolchain: Cow<'static, str>,
    /// The code's file in a plugin's folder, e.g. `code.ppn`.
    pub file: Cow<'static, str>,
    /// This start went without code plugins, and why.
    pub safe_mode: Option<SafeMode>,
    /// Plugins the shell had to install at this start and did not.
    pub not_installed: Vec<NotInstalled>,
}

impl Native {
    /// Code plugins in files named `file`, fit for this build of the SDK.
    pub fn new(file: impl Into<Cow<'static, str>>) -> Self {
        Self {
            fingerprint: FINGERPRINT,
            toolchain: Cow::Borrowed(TOOLCHAIN),
            file: file.into(),
            safe_mode: None,
            not_installed: Vec::new(),
        }
    }

    /// The path of the code of the plugin in `folder`.
    pub fn path(&self, folder: &str) -> String {
        alloc::format!("{}/{}", folder.trim_end_matches('/'), self.file)
    }
}

/// A start without code plugins: when PaperPaw did not start twice with
/// them, or the reader asked for it with a key held at power-on. The code
/// plugins it switched off stay off until switched on again (in the
/// plugin manager), one after the other: one that crashes again is then
/// the one switched off.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SafeMode {
    pub reason: SafeModeReason,
    /// The code plugin loaded last before, if any.
    pub last_loaded: Option<String>,
    /// The code plugin the last crash was in, if it was in one.
    pub crashed: Option<String>,
    /// The code plugins it switched off.
    pub switched_off: Vec<Id>,
}

impl SafeMode {
    pub fn new(reason: SafeModeReason) -> Self {
        Self {
            reason,
            last_loaded: None,
            crashed: None,
            switched_off: Vec::new(),
        }
    }
}

/// Why a start went without code plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SafeModeReason {
    /// The last starts with code plugins did not get to their first
    /// frame.
    FailedStarts,
    /// The reader held the key for it at power-on.
    KeyHeld,
}

/// A plugin the shell had to install at this start (one the web flasher
/// wrote behind the firmware, which the reader chose there) and did not:
/// the plugin manager says which, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NotInstalled {
    pub plugin: Id,
    pub reason: NotInstalledReason,
}

impl NotInstalled {
    pub fn new(plugin: Id, reason: NotInstalledReason) -> Self {
        Self { plugin, reason }
    }
}

/// Why a plugin was not installed at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotInstalledReason {
    /// Its code is signed by another author than the version installed
    /// before ([`Author::Other`]): installed only when the reader says so,
    /// where they are warned (the store).
    OtherAuthor,
    /// Its code is damaged, or no plugin file this PaperPaw reads.
    Damaged,
}

/// The plugin a plugin file's entry made, for the shell that loaded it.
///
/// # Safety
///
/// `made` must be what [`Entry::make`](paperpaw_native::Entry) of a plugin
/// file with this build's [`FINGERPRINT`] returned, taken once.
pub unsafe fn take(made: *mut core::ffi::c_void) -> Option<alloc::boxed::Box<dyn crate::Plugin>> {
    if made.is_null() {
        return None;
    }
    // SAFETY: made by `plugin!` (`support::make`) from this box, with the
    // same layout: the fingerprint says the same compiler and SDK.
    Some(*unsafe {
        alloc::boxed::Box::from_raw(made.cast::<alloc::boxed::Box<dyn crate::Plugin>>())
    })
}

/// Makes a crate a native plugin file: defines its entry, its heap (the
/// kernel's) and its panic handler, around the plugin `$plugin` makes.
/// The crate is `#![no_std]`; for the X4 (`riscv32imc-unknown-none-elf`)
/// and the X4 Pro (`xtensa-esp32s3-none-elf`) a `#![no_main]` binary,
/// linked with `paperpaw-plugin.x` and packed by `paperpaw-sign pack`;
/// where a system loads libraries (a computer, Android) a `cdylib` with
/// the same entry. See the [module documentation](crate::native).
#[macro_export]
macro_rules! plugin {
    ($plugin:expr) => {
        /// The firmware's table, whose address the loader wrote into the
        /// word `paperpaw-plugin.x` makes before any code of the plugin
        /// ran.
        #[cfg(target_os = "none")]
        fn paperpaw_api() -> &'static $crate::native::support::Api {
            unsafe extern "C" {
                static PAPERPAW_API: *const $crate::native::support::Api;
            }
            // SAFETY: a table that lives as long as the firmware runs.
            unsafe { &*PAPERPAW_API }
        }

        /// Where the shell puts its table before it makes the plugin: a
        /// library the system loads may have data of its own.
        #[cfg(not(target_os = "none"))]
        #[unsafe(no_mangle)]
        pub static PAPERPAW_API: $crate::native::support::ApiSlot =
            $crate::native::support::ApiSlot::new();

        #[cfg(not(target_os = "none"))]
        fn paperpaw_api() -> &'static $crate::native::support::Api {
            PAPERPAW_API.get()
        }

        #[global_allocator]
        static PAPERPAW_HEAP: $crate::native::support::Heap =
            $crate::native::support::Heap(paperpaw_api);

        #[panic_handler]
        fn paperpaw_panic(info: &::core::panic::PanicInfo<'_>) -> ! {
            $crate::native::support::panic(paperpaw_api(), info)
        }

        unsafe extern "C" fn paperpaw_make() -> *mut ::core::ffi::c_void {
            $crate::native::support::make($plugin)
        }

        #[unsafe(no_mangle)]
        #[cfg_attr(target_os = "none", unsafe(link_section = ".rodata.paperpaw_entry"))]
        pub static PAPERPAW_ENTRY: $crate::native::support::Entry =
            $crate::native::support::Entry {
                fingerprint: $crate::native::FINGERPRINT.0,
                make: paperpaw_make,
            };
    };
}

/// What [`plugin!`] expands to uses, and what a shell that loads plugins
/// as libraries hands them; not for plugins themselves.
#[doc(hidden)]
pub mod support {
    use alloc::boxed::Box;
    use core::alloc::{GlobalAlloc, Layout};
    use core::fmt::Write;
    use core::sync::atomic::{AtomicPtr, Ordering};

    pub use paperpaw_native::{Api, Entry};

    /// The shell's table, where a library the system loads keeps it
    /// (`PAPERPAW_API`): set before the plugin is made.
    #[derive(Debug)]
    pub struct ApiSlot(AtomicPtr<Api>);

    impl ApiSlot {
        pub const fn new() -> Self {
            Self(AtomicPtr::new(core::ptr::null_mut()))
        }

        /// Gives the plugin the shell's table (the shell, once, before it
        /// makes the plugin).
        pub fn set(&self, api: &'static Api) {
            self.0
                .store(core::ptr::from_ref(api).cast_mut(), Ordering::Release);
        }

        pub fn get(&self) -> &'static Api {
            // SAFETY: set by the shell before it made the plugin, to a
            // table that lives as long as the shell runs.
            unsafe { &*self.0.load(Ordering::Acquire) }
        }
    }

    impl Default for ApiSlot {
        fn default() -> Self {
            Self::new()
        }
    }

    /// The plugin's heap: the kernel's (through the table the function
    /// gives), so boxes cross both ways.
    #[derive(Debug)]
    pub struct Heap(pub fn() -> &'static Api);

    // SAFETY: the firmware's allocator, which keeps `GlobalAlloc`'s rules.
    unsafe impl GlobalAlloc for Heap {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: as `GlobalAlloc::alloc`.
            unsafe { (self.0().alloc)(layout.size(), layout.align()) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: as `GlobalAlloc::dealloc`.
            unsafe { (self.0().dealloc)(ptr, layout.size(), layout.align()) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            // SAFETY: as `GlobalAlloc::realloc`.
            unsafe { (self.0().realloc)(ptr, layout.size(), layout.align(), new_size) }
        }
    }

    /// The plugin made, as [`take`](super::take) reads it back.
    pub fn make(plugin: impl crate::Plugin + 'static) -> *mut core::ffi::c_void {
        let plugin: Box<dyn crate::Plugin> = Box::new(plugin);
        Box::into_raw(Box::new(plugin)).cast()
    }

    /// Writes into a fixed buffer, cutting what does not fit.
    struct Cut {
        buf: [u8; 192],
        len: usize,
    }

    impl Write for Cut {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for c in s.chars() {
                let mut bytes = [0; 4];
                let bytes = c.encode_utf8(&mut bytes).as_bytes();
                if self.len + bytes.len() > self.buf.len() {
                    break;
                }
                self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
                self.len += bytes.len();
            }
            Ok(())
        }
    }

    /// The plugin panicked: the firmware's panic, with what it says.
    pub fn panic(api: &Api, info: &core::panic::PanicInfo<'_>) -> ! {
        let mut cut = Cut {
            buf: [0; 192],
            len: 0,
        };
        let _ = write!(cut, "{}", info.message());
        if let Some(at) = info.location() {
            let file = at.file().rsplit(['/', '\\']).next().unwrap_or(at.file());
            let _ = write!(cut, " ({file}:{})", at.line());
        }
        // SAFETY: the firmware's table; the message lives on the stack
        // until the firmware has taken it.
        unsafe { (api.panic)(cut.buf.as_ptr(), cut.len) }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn what_cannot_be_read_is_not_written_over() {
        let kept = crate::MemoryStorage::new();
        assert!(remember_author(&kept, "clock", &[1; 32]));
        assert!(remember_code(&kept, "clock", &[2; 32]));
        let authors = crate::read_file(&kept, AUTHORS, 1024).unwrap();
        let codes = crate::read_file(&kept, INSTALLED, 1024).unwrap();
        // The card fails: nothing kept, the other plugins' left as they were.
        let failing = crate::memory_storage::Unreadable(kept);
        assert!(!remember_author(&failing, "weather", &[3; 32]));
        assert!(!remember_code(&failing, "weather", &[4; 32]));
        assert_eq!(
            crate::read_file(&failing.0, AUTHORS, 1024).unwrap(),
            authors
        );
        assert_eq!(
            crate::read_file(&failing.0, INSTALLED, 1024).unwrap(),
            codes
        );
        // A file too large to read is not written over either.
        let large = crate::MemoryStorage::new().with(INSTALLED, "x".repeat(70 * 1024));
        assert!(!remember_code(&large, "clock", &[2; 32]));
        assert_eq!(large.size(INSTALLED), Ok(70 * 1024));
    }

    #[test]
    fn code_is_named_as_installed() {
        let storage = crate::MemoryStorage::new();
        assert_eq!(installed_code(&storage, "clock"), None);
        assert!(remember_code(&storage, "clock", &[1; 32]));
        assert!(remember_author(&storage, "clock", &[2; 32]));
        assert!(remember_code(&storage, "clock", &[3; 32]));
        assert_eq!(installed_code(&storage, "clock"), Some([3; 32]));
        // Kept apart from the authors.
        assert_eq!(known_author(&storage, "clock"), Some([2; 32]));
        assert_eq!(installed_code(&storage, "clock-face"), None);
    }

    #[test]
    fn authors_are_known_by_plugin() {
        let storage = crate::MemoryStorage::new();
        assert_eq!(known_author(&storage, "clock"), None);
        assert_eq!(author(&storage, "clock", &[1; 32]), Author::New);
        assert!(remember_author(&storage, "clock", &[1; 32]));
        assert!(remember_author(&storage, "clock-face", &[2; 32]));
        assert_eq!(author(&storage, "clock", &[1; 32]), Author::Known);
        assert_eq!(author(&storage, "clock", &[2; 32]), Author::Other);
        // Another author installed anyway: the one from now on.
        assert!(remember_author(&storage, "clock", &[3; 32]));
        assert_eq!(known_author(&storage, "clock"), Some([3; 32]));
        assert_eq!(known_author(&storage, "clock-face"), Some([2; 32]));
    }

    #[test]
    fn fingerprints_as_text() {
        let fingerprint = Fingerprint(0x0085_073a_6132_3717);
        assert_eq!(fingerprint.to_string(), "0085073a61323717");
        assert_eq!(Fingerprint::parse("0085073a61323717"), Some(fingerprint));
        assert_eq!(Fingerprint::parse(" 0085073A61323717\n"), Some(fingerprint));
        assert_eq!(Fingerprint::parse("85073a61323717"), None);
        assert_eq!(Fingerprint::parse("0085073a6132371g"), None);
        assert_eq!(Fingerprint::parse("+085073a61323717"), None);
    }

    /// A small plugin file of `Header`'s own tests: no code to run, only
    /// to check.
    fn file() -> Vec<u8> {
        let mut header = Header {
            chip: Chip::Esp32C3,
            fingerprint: 1,
            base: paperpaw_native::LINK_BASE,
            image_len: 64,
            code_at: 32,
            entry: 0,
            api: 16,
            relocs: 0,
            align: 16,
            sha256: [0; 32],
        };
        let image = [9u8; 64];
        let mut sha = header.digest();
        sha.update(&image);
        header.sha256 = sha.finish();
        let mut file = header.to_bytes().to_vec();
        file.extend_from_slice(&image);
        file
    }

    #[test]
    fn plugin_files_on_the_storage_are_checked_whole() {
        let storage = crate::MemoryStorage::new();
        let unsigned = file();
        let mut signed = unsigned.clone();
        let author = signature::sign(&unsigned, &[2; 32]);
        signed.extend_from_slice(&author.to_bytes());
        storage.write("a.ppn", &unsigned).unwrap();
        storage.write("b.ppn", &signed).unwrap();
        assert_eq!(check_file(&storage, "a.ppn").map(|(_, s)| s), Ok(None));
        assert_eq!(
            check_file(&storage, "b.ppn").map(|(_, s)| s),
            Ok(Some(author))
        );
        let mut changed = signed.clone();
        changed[100] ^= 1;
        storage.write("c.ppn", &changed).unwrap();
        assert_eq!(check_file(&storage, "c.ppn"), Err(Problem::Damaged));
        let mut resigned = signed.clone();
        resigned[signed.len() - 30] ^= 1;
        storage.write("d.ppn", &resigned).unwrap();
        assert_eq!(check_file(&storage, "d.ppn"), Err(Problem::Signature));
        storage.write("e.ppn", &signed[..signed.len() - 1]).unwrap();
        assert_eq!(check_file(&storage, "e.ppn"), Err(Problem::Length));
        assert_eq!(check_file(&storage, "none.ppn"), Err(Problem::Length));
    }

    /// The web flasher's bundle installs only what `install` would: the
    /// same rules, kept in two places as the format crate is below the SDK.
    #[test]
    fn bundles_take_the_names_install_takes() {
        use paperpaw_native::bundle::{is_plugin_id, is_safe_path};
        let ids = [
            "a",
            "reading-nook",
            "a1",
            "",
            "-a",
            "a-",
            "A",
            "a.b",
            "a_b",
            "\u{e9}",
        ];
        let long_id = "a".repeat(41);
        for id in ids.iter().copied().chain([long_id.as_str(), &long_id[1..]]) {
            assert_eq!(
                is_plugin_id(id),
                crate::install::is_plugin_name(id),
                "{id:?}"
            );
        }
        let paths = [
            "plugin.ini",
            "i18n/de.ini",
            "fonts/A_b-c.ttf",
            "",
            "/a",
            "a/",
            "a//b",
            "../a",
            ".a",
            "a/.b",
            "a b",
            "a\\b",
            "a:b",
        ];
        let long_path = "a".repeat(101);
        for path in paths
            .iter()
            .copied()
            .chain([long_path.as_str(), &long_path[1..]])
        {
            assert_eq!(
                is_safe_path(path),
                crate::install::is_safe_path(path),
                "{path:?}"
            );
        }
    }

    #[test]
    fn this_build_names_itself() {
        let native = Native::new("code.ppn");
        assert_eq!(native.fingerprint, FINGERPRINT);
        assert!(
            native.toolchain.starts_with("rustc "),
            "{}",
            native.toolchain
        );
        assert_eq!(native.path("plugins/hello/"), "plugins/hello/code.ppn");
    }
}
