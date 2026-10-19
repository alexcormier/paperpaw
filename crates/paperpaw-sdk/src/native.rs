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
}

impl Native {
    /// Code plugins in files named `file`, fit for this build of the SDK.
    pub fn new(file: impl Into<Cow<'static, str>>) -> Self {
        Self {
            fingerprint: FINGERPRINT,
            toolchain: Cow::Borrowed(TOOLCHAIN),
            file: file.into(),
            safe_mode: None,
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
    fn fingerprints_as_text() {
        let fingerprint = Fingerprint(0x0085_073a_6132_3717);
        assert_eq!(fingerprint.to_string(), "0085073a61323717");
        assert_eq!(Fingerprint::parse("0085073a61323717"), Some(fingerprint));
        assert_eq!(Fingerprint::parse(" 0085073A61323717\n"), Some(fingerprint));
        assert_eq!(Fingerprint::parse("85073a61323717"), None);
        assert_eq!(Fingerprint::parse("0085073a6132371g"), None);
        assert_eq!(Fingerprint::parse("+085073a61323717"), None);
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
