//! Files of settings the kernel keeps (`paperpaw.ini`, a book's own
//! settings, passwords and keys): read as they are, never written over
//! where they could not be read.

use super::*;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::builtin::CONFIG_FILE;

/// A storage whose reads of one file fail, as a failing card's do.
struct FailingReads {
    inner: MemoryStorage,
    path: &'static str,
}

impl Storage for FailingReads {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path == self.path {
            return Err(StorageError::Io);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
}

/// The bytes of `path`, read past every cache.
fn bytes(kernel: &Kernel, path: &str) -> Vec<u8> {
    kernel.shared.recall.clear();
    paperpaw_sdk::read_file(kernel.storage(), path, 1 << 20).unwrap_or_default()
}

fn kernel_with(storage: impl Storage + 'static) -> Kernel {
    Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: log() })
        .storage(storage)
        .build()
        .unwrap()
}

/// Every way a stored `paperpaw.ini` can be there but unreadable.
fn unreadable_configs() -> Vec<(&'static str, Box<dyn Storage>)> {
    let mut large = String::from("[core]\nui-size = 12\n");
    while large.len() <= 70 * 1024 {
        large.push_str("# a comment that makes the file longer than is read of it\n");
    }
    let latin1 = b"# Gr\xFC\xDFe\n[core]\nui-size = 12\n".to_vec();
    let failing = FailingReads {
        inner: MemoryStorage::new().with(CONFIG_FILE, "[core]\nui-size = 12\n"),
        path: CONFIG_FILE,
    };
    let latin1: Box<dyn Storage> = Box::new(MemoryStorage::new().with(CONFIG_FILE, latin1));
    alloc::vec![
        ("not UTF-8", latin1),
        (
            "too large",
            Box::new(MemoryStorage::new().with(CONFIG_FILE, large))
        ),
        ("failing reads", Box::new(failing)),
    ]
}

/// A `paperpaw.ini` that is there but cannot be read is said at start
/// and to the user, and no change writes over it: settings, bindings,
/// menus, plugins switched, a setting reset.
#[test]
fn a_config_that_cannot_be_read_is_said_and_left_as_it_is() {
    for (case, storage) in unreadable_configs() {
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(NoticePlugin { log: log() })
            .storage_boxed(storage)
            .build()
            .unwrap();
        let before = bytes(&kernel, CONFIG_FILE);
        let warnings = kernel.take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.source == WarningSource::UserConfig && w.message.contains(CONFIG_FILE)),
            "{case}: {warnings:?}"
        );
        kernel.start(Instant(0));
        assert!(kernel.notice.is_some(), "{case}: the user is told");
        kernel
            .set_setting(Instant(10), "core.night-mode", "on")
            .unwrap();
        // It applies, only it is not saved.
        assert!(kernel.settings().bool("core.night-mode"), "{case}");
        kernel.apply(Command::Bind {
            layer: String::from("global"),
            input: String::from("button.menu"),
            action: Some(actions::TOGGLE_NIGHT_MODE),
        });
        kernel.apply(Command::SetMenu(
            Id::new_static("core.main"),
            Some(alloc::vec![actions::BACK]),
        ));
        kernel.apply(Command::ResetSetting(Id::new_static("core.ui-size")));
        kernel.finish();
        assert_eq!(bytes(&kernel, CONFIG_FILE), before, "{case}: written over");
        assert!(
            kernel
                .take_warnings()
                .iter()
                .any(|w| w.message.contains("cannot be read")),
            "{case}"
        );
        // The binding applies at once all the same.
        assert_eq!(
            kernel.resolve(&crate::keymap::InputKey::Button(Cow::Borrowed("menu"))),
            Some(&actions::TOGGLE_NIGHT_MODE),
            "{case}"
        );
    }
}

/// A file saved as "UTF-8 with BOM" (Windows Notepad) is read whole and
/// changed as it is.
#[test]
fn a_config_with_a_byte_order_mark_is_read_whole() {
    let config = "\u{FEFF}[core]\nui-size = 12\n[keymap.global]\nbutton.up = core.menu\n";
    let mut kernel = kernel_with(MemoryStorage::new().with(CONFIG_FILE, config));
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert!(kernel.take_warnings().is_empty());
    kernel.start(Instant(0));
    kernel
        .set_setting(Instant(10), "core.night-mode", "on")
        .unwrap();
    let saved = String::from_utf8(bytes(&kernel, CONFIG_FILE)).unwrap();
    assert_eq!(
        saved,
        "[core]\nui-size = 12\nnight-mode = on\n[keymap.global]\nbutton.up = core.menu\n"
    );
}

/// Binding a button in PaperPaw keeps the shell's own bindings (the
/// desktop's user config) above the user's.
#[test]
fn bindings_made_keep_the_shells_own() {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .user_config("[keymap.global]\nbutton.up = core.home\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let key = |name: &'static str| crate::keymap::InputKey::Button(Cow::Borrowed(name));
    assert_eq!(kernel.resolve(&key("up")), Some(&actions::HOME));
    kernel.apply(Command::Bind {
        layer: String::from("global"),
        input: String::from("button.menu"),
        action: Some(actions::TOGGLE_NIGHT_MODE),
    });
    kernel.finish();
    assert_eq!(
        kernel.resolve(&key("menu")),
        Some(&actions::TOGGLE_NIGHT_MODE)
    );
    assert_eq!(kernel.resolve(&key("up")), Some(&actions::HOME), "kept");
}

const BOOK_SETTINGS: &str = "books/a/settings.ini";

/// A kernel with "only this book" on for the book whose settings are
/// `source`.
fn scoped_kernel(source: &[u8]) -> Kernel {
    let storage = MemoryStorage::new().with(BOOK_SETTINGS, source.to_vec());
    let mut kernel = kernel_with(storage);
    kernel.start(Instant(0));
    kernel.set_scope(Some(String::from(BOOK_SETTINGS)));
    kernel
}

/// A book's settings file that cannot be read is not written over.
#[test]
fn a_books_settings_that_cannot_be_read_are_left_as_they_are() {
    let source = b"[core]\nscope-only = on\n# Gr\xFC\xDFe\nreading-font = core.default\n";
    let mut kernel = scoped_kernel(source);
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains(BOOK_SETTINGS))
    );
    kernel
        .set_setting(Instant(10), "core.scope-only", "on")
        .unwrap();
    assert_eq!(bytes(&kernel, BOOK_SETTINGS), source);
}

/// Saving a book's settings keeps what else its file holds, e.g. the
/// settings of a plugin switched off, and comments.
#[test]
fn a_books_settings_keep_what_else_they_hold() {
    let source = "# mine\n[gone]\nsize = 3\n[core]\nscope-only = on\n";
    let mut kernel = scoped_kernel(source.as_bytes());
    kernel
        .set_setting(Instant(10), "core.font-size", "14")
        .unwrap();
    let saved = String::from_utf8(bytes(&kernel, BOOK_SETTINGS)).unwrap();
    assert!(saved.starts_with("# mine\n[gone]\nsize = 3\n"), "{saved}");
    assert!(saved.contains("font-size = 14"), "{saved}");
    // "Only this book" off: the scoped values go, the rest stays.
    kernel
        .set_setting(Instant(20), "core.scope-only", "off")
        .unwrap();
    let saved = String::from_utf8(bytes(&kernel, BOOK_SETTINGS)).unwrap();
    assert!(saved.contains("size = 3"), "{saved}");
}

/// The passwords and keys file that cannot be read is not written over.
#[test]
fn passwords_that_cannot_be_read_are_left_as_they_are() {
    use paperpaw_sdk::secrets::{FILE, SecretChange};
    let source = b"wifi.home\tHome\t\xFF\xFE\n".to_vec();
    let mut kernel = kernel_with(MemoryStorage::new().with(FILE, source.clone()));
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains(FILE))
    );
    kernel.start(Instant(0));
    kernel.set_secret(SecretChange::keep("a.key", "a.label", "hunter2"));
    assert_eq!(bytes(&kernel, FILE), source);
}
