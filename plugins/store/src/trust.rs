//! Whether a plugin downloaded may be installed (ADR 0024). A data
//! plugin may. A code plugin runs with full access to the device, so its
//! code is checked whole first ([`check_file`]): made for this PaperPaw,
//! and signed by its author, whose key is remembered at the first install
//! ([`native::author`]). At the first install the key must be the
//! one the library names for the plugin; later it must be the one
//! remembered, and an update signed by another waits until the reader,
//! warned, says to install it anyway.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::install::Progress;
use paperpaw_sdk::native::{self, Author, check_file};
use paperpaw_sdk::{Command, Env, Storage};

use crate::INSTALLER;

/// Why a plugin downloaded is not installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Its code is damaged, or no plugin file this PaperPaw reads.
    Damaged,
    /// Its code was built for another PaperPaw or device.
    OtherBuild,
    /// Its code is not signed.
    Unsigned,
    /// Its code is signed by another key than the library names.
    NotTheAuthor,
    /// This PaperPaw loads no code plugins.
    Unsupported,
}

impl Refusal {
    /// What the reader is told, a translation key taking `{name}`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Refusal::Damaged => "store.refused.damaged",
            Refusal::OtherBuild => "store.refused.other-build",
            Refusal::Unsigned => "store.refused.unsigned",
            Refusal::NotTheAuthor => "store.refused.not-the-author",
            Refusal::Unsupported => "store.refused.unsupported",
        }
    }
}

/// What to do with a plugin all of whose files are here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Install it; for a code plugin, with its author's key to remember.
    Install {
        author: Option<[u8; 32]>,
    },
    /// Its code is signed by `author`, another than the author of the
    /// version installed before: only if the reader says so.
    OtherAuthor {
        author: [u8; 32],
    },
    Refused(Refusal),
}

/// The key the library names for `plugin`'s author, kept from when its
/// download started until it is installed.
fn expected_path() -> String {
    format!("{}/author.txt", Env::plugin_dir(crate::PLUGIN.as_str()))
}

/// Keeps the key the library names for the author of `plugin`, which is
/// being downloaded (`None` for a data plugin).
pub(crate) fn expect(storage: &dyn Storage, plugin: &str, key: Option<&[u8; 32]>) {
    match key {
        Some(key) => {
            let mut text = String::from(plugin);
            text.push('\t');
            for byte in key {
                text.push_str(&format!("{byte:02x}"));
            }
            let _ = storage.write(&expected_path(), text.as_bytes());
        }
        None => {
            let _ = storage.remove(&expected_path());
        }
    }
}

fn expected(storage: &dyn Storage, plugin: &str) -> Option<[u8; 32]> {
    let text = paperpaw_sdk::read_file(storage, &expected_path(), 256).ok()?;
    let text = core::str::from_utf8(&text).ok()?;
    let (id, hex) = text.trim().split_once('\t')?;
    if id != plugin || hex.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (byte, pair) in key.iter_mut().zip(hex.as_bytes().chunks(2)) {
        let pair = core::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(key)
}

/// Whether the plugin downloaded into `folder` may be installed.
pub(crate) fn verdict(env: &Env<'_>, plugin: &str, folder: &str) -> Verdict {
    let storage = env.storage;
    let code = paperpaw_sdk::native::CODE_FILES
        .iter()
        .map(|name| format!("{folder}/{name}"))
        .find(|path| storage.exists(path));
    let Some(code) = code else {
        return Verdict::Install { author: None };
    };
    let Some(native) = env.native.filter(|native| code.ends_with(&*native.file)) else {
        return Verdict::Refused(Refusal::Unsupported);
    };
    let (header, signature) = match check_file(storage, &code) {
        Ok(checked) => checked,
        Err(_) => return Verdict::Refused(Refusal::Damaged),
    };
    if header.fingerprint != native.fingerprint.0 {
        return Verdict::Refused(Refusal::OtherBuild);
    }
    let Some(signature) = signature else {
        return Verdict::Refused(Refusal::Unsigned);
    };
    let key = signature.key;
    match native::author(storage, plugin, &key) {
        Author::Known => Verdict::Install { author: Some(key) },
        Author::Other => Verdict::OtherAuthor { author: key },
        Author::New if expected(storage, plugin) == Some(key) => {
            Verdict::Install { author: Some(key) }
        }
        Author::New => Verdict::Refused(Refusal::NotTheAuthor),
    }
}

/// Installs the complete job's plugin, remembering its author's key; for
/// one signed by another author, after the reader said to.
pub(crate) fn install(
    env: &Env<'_>,
    author: Option<[u8; 32]>,
    out: &mut Vec<Command>,
) -> Option<Progress> {
    let progress = INSTALLER.progress(env.storage)?;
    if let Some(key) = author {
        native::remember_author(env.storage, &progress.plugin, &key);
    }
    let _ = env.storage.remove(&expected_path());
    INSTALLER.install(env, out)
}

/// Installs the plugin waiting for the reader's word, signed by another
/// author than the one before: that author is remembered from now on.
pub(crate) fn install_anyway(env: &Env<'_>, out: &mut Vec<Command>) -> Option<Progress> {
    let progress = INSTALLER.progress(env.storage)?;
    let folder = INSTALLER.folder(env.storage)?;
    match verdict(env, &progress.plugin, &folder) {
        Verdict::OtherAuthor { author } => install(env, Some(author), out),
        _ => None,
    }
}

/// Whether the reader was told once what code plugins are, before the
/// first one is installed.
fn told_path() -> String {
    format!("{}/code-told", Env::plugin_dir(crate::PLUGIN.as_str()))
}

pub(crate) fn told(storage: &dyn Storage) -> bool {
    storage.exists(&told_path())
}

pub(crate) fn tell(storage: &dyn Storage) {
    let _ = storage.create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
    let _ = storage.write(&told_path(), b"");
}
