//! PaperPaw's storage on a FAT file system (FAT12, FAT16, FAT32) with long
//! file names, e.g. the X4's SD card: [`FatStorage`] over any disk that
//! reads, writes and seeks in bytes (see [`fatfs::ReadWriteSeek`]), such
//! as a [`BlockDisk`] over an SD card's blocks.
//!
//! The [`OPEN_FILES`] files read last stay open, so reading a book a piece
//! at a time, often from two files by turns (an index and its text), does
//! not look them up again for every piece. Files appended to stay open
//! the same way, written back only when they are wanted otherwise (listed,
//! renamed, removed, replaced), when they make room for another file, or on
//! [`Storage::sync`]: a picture decoded into a file in hundreds of pieces
//! is not looked up and written back hundreds of times. A [`BlockDisk`]
//! keeps changed blocks too (folders, the FAT): shared with the storage
//! ([`SharedDisk`], [`FatStorage::with_disk`]), `sync` writes them back
//! as well, so nothing written is lost when the device is cut off after. Files are replaced
//! through a temporary file beside them, so a write cut short by an empty
//! battery leaves the old content.
//!
//! The [`FOLDERS`] folders looked up last are kept too: a file opened in
//! one is looked up there, not by walking every folder from the root again
//! (a book's data lies five folders deep, beside those of all other books).
//!
//! A name stands for an entry by its long name where it has one, by its
//! short name only where it has none: `PAPERP~1`, the short name FAT
//! keeps beside `.paperpaw`, does not reach PaperPaw's data. rust-fatfs
//! takes either, so names are looked up here (`lookup`) and handed to it
//! only where it cannot take another entry for them.

#![no_std]

extern crate alloc;

mod disk;

pub use disk::{BLOCK, BlockDisk, Blocks, DiskError, SharedDisk, VolumeError};

use alloc::format;
use alloc::string::{String, ToString};
use core::cell::RefCell;

use fatfs::{
    DefaultTimeProvider, Dir, File, FileSystem, LossyOemCpConverter, Read, ReadWriteSeek, Seek,
    SeekFrom, Write,
};
use paperpaw_platform::storage::is_valid_path;
use paperpaw_platform::{DirEntry, Storage, StorageError};

pub use fatfs;

type FatFile<IO> = File<'static, IO, DefaultTimeProvider, LossyOemCpConverter>;

/// Files kept open (each takes about a hundred bytes).
pub const OPEN_FILES: usize = 16;
/// Folders kept (each takes about a hundred bytes).
pub const FOLDERS: usize = 8;
type FatDir<IO> = Dir<'static, IO, DefaultTimeProvider, LossyOemCpConverter>;
type FatEntry<IO> = fatfs::DirEntry<'static, IO, DefaultTimeProvider, LossyOemCpConverter>;

/// A file kept open.
struct Open<IO: ReadWriteSeek + 'static> {
    path: String,
    file: FatFile<IO>,
    /// Appended to since it was last written back.
    dirty: bool,
}

impl<IO: ReadWriteSeek + 'static> Open<IO> {
    /// Writes back what was appended (its size in its folder, the disk's
    /// blocks).
    fn flush(&mut self) -> Result<(), StorageError> {
        if self.dirty {
            self.dirty = false;
            self.file.flush().map_err(error)?;
        }
        Ok(())
    }
}

/// PaperPaw's storage on a FAT file system that lives as long as the
/// device runs (`Box::leak` it).
pub struct FatStorage<IO: ReadWriteSeek + 'static> {
    fs: &'static FileSystem<IO>,
    /// The files used last, the latest last.
    open: RefCell<alloc::vec::Vec<Open<IO>>>,
    /// The folders looked up last, the latest last, by their paths.
    folders: RefCell<alloc::vec::Vec<(String, FatDir<IO>)>>,
    /// Writes back the disk's changed blocks, where the disk is shared.
    flush_disk: Option<alloc::boxed::Box<dyn Fn() -> bool>>,
}

impl<IO: ReadWriteSeek + 'static> core::fmt::Debug for FatStorage<IO> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FatStorage").finish_non_exhaustive()
    }
}

fn error<E>(e: fatfs::Error<E>) -> StorageError {
    match e {
        fatfs::Error::NotFound => StorageError::NotFound,
        fatfs::Error::NotEnoughSpace => StorageError::Full,
        fatfs::Error::InvalidInput
        | fatfs::Error::InvalidFileNameLength
        | fatfs::Error::UnsupportedFileNameCharacter => StorageError::InvalidPath,
        _ => StorageError::Io,
    }
}

fn check(path: &str) -> Result<(), StorageError> {
    if is_valid_path(path) {
        Ok(())
    } else {
        Err(StorageError::InvalidPath)
    }
}

/// The folder of `path` (`""` for the root).
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The name of `path` in its folder.
fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Whether `stored` is `name` to FAT: without regard to case, compared as
/// rust-fatfs compares names (in upper case, letter by letter).
fn same_name(stored: impl Iterator<Item = char>, name: &str) -> bool {
    stored
        .flat_map(char::to_uppercase)
        .eq(name.chars().flat_map(char::to_uppercase))
}

/// Whether `entry`'s long name is `name`; `None` where it has none.
fn long_name_is<IO: ReadWriteSeek + 'static>(entry: &FatEntry<IO>, name: &str) -> Option<bool> {
    let units = entry.long_file_name_as_ucs2_units()?;
    let long =
        char::decode_utf16(units.iter().copied()).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER));
    Some(same_name(long, name))
}

/// Whether `entry`'s short name is `name`, its letters read as
/// [`LossyOemCpConverter`] reads them.
fn short_name_is<IO: ReadWriteSeek + 'static>(entry: &FatEntry<IO>, name: &str) -> bool {
    let short = entry.short_file_name_as_bytes().iter().map(|&b| {
        if b.is_ascii() {
            char::from(b)
        } else {
            char::REPLACEMENT_CHARACTER
        }
    });
    same_name(short, name)
}

/// Most entries a folder holds (FAT's limit). Read on past them, a
/// folder whose clusters run in a loop (a damaged card) would never end.
/// Not where the loop holds removed entries only: rust-fatfs reads past
/// those itself, giving none.
const ENTRIES: usize = 65_536;

/// `items`, failing past [`ENTRIES`].
fn bounded<T>(
    items: impl Iterator<Item = Result<T, StorageError>>,
) -> impl Iterator<Item = Result<T, StorageError>> {
    items.enumerate().map(|(i, item)| match i {
        ..ENTRIES => item,
        _ => Err(StorageError::Io),
    })
}

/// The entries of `dir`, failing past [`ENTRIES`].
fn entries<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
) -> impl Iterator<Item = Result<FatEntry<IO>, StorageError>> {
    bounded(dir.iter().map(|entry| entry.map_err(error)))
}

/// What a name stands for in a folder.
struct Lookup<IO: ReadWriteSeek + 'static> {
    /// The entry it names: by its long name, or by its short name where
    /// it has no long one.
    entry: Option<FatEntry<IO>>,
    /// rust-fatfs could take another entry for the name (one whose short
    /// name it is, as `PAPERP~1` for `.paperpaw`): the name is not handed
    /// to rust-fatfs then.
    others: bool,
}

/// Looks up `name` in `dir`, as PaperPaw names entries (see the [crate
/// documentation](crate)): every entry, where the name goes to rust-fatfs
/// after (`whole`), else up to the one it names.
fn lookup<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    whole: bool,
) -> Result<Lookup<IO>, StorageError> {
    let mut found = Lookup {
        entry: None,
        others: false,
    };
    for entry in entries(dir) {
        let entry = entry?;
        let by_short = short_name_is(&entry, name);
        let (named, other) = match long_name_is(&entry, name) {
            Some(by_long) => (by_long, by_short),
            None => (by_short, false),
        };
        if named && found.entry.is_none() {
            found.entry = Some(entry);
            if !whole {
                break;
            }
        } else if named || other {
            found.others = true;
        }
    }
    Ok(found)
}

/// Folder `name` in `dir`.
fn open_dir<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<FatDir<IO>, StorageError> {
    match lookup(dir, name, false)?.entry {
        Some(entry) if entry.is_dir() => Ok(entry.to_dir()),
        Some(_) => Err(StorageError::InvalidPath),
        None => Err(StorageError::NotFound),
    }
}

/// File `name` in `dir`.
fn open_file<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<FatFile<IO>, StorageError> {
    match lookup(dir, name, false)?.entry {
        Some(entry) if entry.is_file() => Ok(entry.to_file()),
        Some(_) => Err(StorageError::InvalidPath),
        None => Err(StorageError::NotFound),
    }
}

/// File `name` in `dir`, made if it is not there. Not where `name` is
/// only another entry's short name: rust-fatfs would open that entry.
fn create_file<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<FatFile<IO>, StorageError> {
    match lookup(dir, name, true)? {
        Lookup {
            entry: Some(entry), ..
        } if entry.is_file() => Ok(entry.to_file()),
        Lookup { entry: Some(_), .. } | Lookup { others: true, .. } => {
            Err(StorageError::InvalidPath)
        }
        Lookup { .. } => dir.create_file(name).map_err(error),
    }
}

/// Folder `name` in `dir`, made if it is not there (as [`create_file`]).
fn create_dir_in<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<FatDir<IO>, StorageError> {
    match lookup(dir, name, true)? {
        Lookup {
            entry: Some(entry), ..
        } if entry.is_dir() => Ok(entry.to_dir()),
        Lookup { entry: Some(_), .. } | Lookup { others: true, .. } => {
            Err(StorageError::InvalidPath)
        }
        Lookup { .. } => dir.create_dir(name).map_err(error),
    }
}

/// Removes `name` from `dir` (a file, or an empty folder); nothing to do
/// where it is not there.
fn remove_in<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<(), StorageError> {
    match lookup(dir, name, true)? {
        Lookup { entry: None, .. } => Ok(()),
        Lookup { others: true, .. } => Err(StorageError::InvalidPath),
        Lookup { .. } => match dir.remove(name) {
            Ok(()) | Err(fatfs::Error::NotFound) => Ok(()),
            Err(e) => Err(error(e)),
        },
    }
}

/// Moves `name` of `dir` to `to_name` in `to`, where nothing is.
fn rename_in<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    to: &FatDir<IO>,
    to_name: &str,
) -> Result<(), StorageError> {
    match lookup(dir, name, true)? {
        Lookup { entry: None, .. } => return Err(StorageError::NotFound),
        Lookup { others: true, .. } => return Err(StorageError::InvalidPath),
        Lookup { .. } => {}
    }
    match lookup(to, to_name, true)? {
        Lookup {
            entry: None,
            others: false,
        } => dir.rename(name, to, to_name).map_err(error),
        Lookup { .. } => Err(StorageError::InvalidPath),
    }
}

impl<IO: ReadWriteSeek + 'static> FatStorage<IO> {
    pub fn new(fs: &'static FileSystem<IO>) -> Self {
        Self {
            fs,
            open: RefCell::new(alloc::vec::Vec::new()),
            folders: RefCell::new(alloc::vec::Vec::new()),
            flush_disk: None,
        }
    }

    /// The same, with the file system's disk shared ([`SharedDisk`]):
    /// [`Storage::sync`] writes back its changed blocks too.
    pub fn with_disk<B: Blocks + 'static>(mut self, disk: SharedDisk<B>) -> Self {
        self.flush_disk = Some(alloc::boxed::Box::new(move || disk.flush_all().is_ok()));
        self
    }

    /// Whether a file or a folder is at `path`, looked up in its folder
    /// (no file opened).
    fn is_there(&self, path: &str) -> bool {
        self.dir(parent(path))
            .and_then(|dir| lookup(&dir, name(path), false))
            .is_ok_and(|found| found.entry.is_some())
    }

    fn root(&self) -> FatDir<IO> {
        self.fs.root_dir()
    }

    /// Folder `path`: kept from before, or looked up from the root and
    /// kept.
    fn dir(&self, path: &str) -> Result<FatDir<IO>, StorageError> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(self.root());
        }
        {
            let mut folders = self.folders.borrow_mut();
            if let Some(i) = folders.iter().position(|(p, _)| p == path) {
                // The latest last.
                let folder = folders.remove(i);
                let dir = folder.1.clone();
                folders.push(folder);
                return Ok(dir);
            }
        }
        // Looked up in its folder, kept too: the folders of many books'
        // data are not each looked up from the root.
        let dir = open_dir(&self.dir(parent(path))?, name(path))?;
        let mut folders = self.folders.borrow_mut();
        if folders.len() == FOLDERS {
            folders.remove(0);
        }
        folders.push((path.to_string(), dir.clone()));
        Ok(dir)
    }

    /// Forgets the folders kept at `path` or below it (removed, renamed).
    fn forget(&self, path: &str) {
        let path = path.trim_matches('/');
        let below = format!("{path}/");
        self.folders
            .borrow_mut()
            .retain(|(p, _)| p != path && !p.starts_with(&below));
    }

    /// Runs `f` with `path` open (created first with its folders if
    /// `create`), keeping it open for the next call.
    fn with_open<T>(
        &self,
        path: &str,
        create: bool,
        f: impl FnOnce(&mut Open<IO>) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let mut open = self.open.borrow_mut();
        match open.iter().position(|o| o.path == path) {
            // The latest last.
            Some(i) => {
                let file = open.remove(i);
                open.push(file);
            }
            None => {
                // Checked once, when opened: a book is read 4 bytes at a
                // time, many times a page.
                check(path)?;
                if open.len() == OPEN_FILES {
                    open.remove(0).flush()?;
                }
                let file = if create {
                    self.create_dir(parent(path))?;
                    create_file(&self.dir(parent(path))?, name(path))
                } else {
                    open_file(&self.dir(parent(path))?, name(path))
                }?;
                open.push(Open {
                    path: path.to_string(),
                    file,
                    dirty: false,
                });
            }
        }
        match open.last_mut() {
            Some(file) => f(file),
            None => Err(StorageError::Io),
        }
    }

    /// Closes the open files at `path` (or below it), writing back what
    /// was appended to them.
    fn close(&self, path: &str) -> Result<(), StorageError> {
        let below = format!("{path}/");
        let mut result = Ok(());
        self.open.borrow_mut().retain_mut(|o| {
            let keep = o.path != path && !o.path.starts_with(&below);
            if !keep && let Err(e) = o.flush() {
                result = Err(e);
            }
            keep
        });
        result
    }

    fn write_all(file: &mut FatFile<IO>, data: &[u8]) -> Result<(), StorageError> {
        file.write_all(data).map_err(error)?;
        file.flush().map_err(error)
    }
}

impl<IO: ReadWriteSeek + 'static> Storage for FatStorage<IO> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.with_open(path, false, |Open { file, .. }| {
            file.seek(SeekFrom::Start(offset)).map_err(error)?;
            let mut read = 0;
            while read < buf.len() {
                match file.read(&mut buf[read..]).map_err(error)? {
                    0 => break,
                    n => read += n,
                }
            }
            Ok(read)
        })
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.with_open(path, false, |o| {
            o.file.seek(SeekFrom::End(0)).map_err(error)
        })
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        check(path)?;
        self.close(path)?;
        self.create_dir(parent(path))?;
        let dir = self.dir(parent(path))?;
        let (name, temp) = (name(path), format!("{}.tmp", name(path)));
        // Refused before anything is written: no temporary file is left.
        if lookup(&dir, name, true)?.others {
            return Err(StorageError::InvalidPath);
        }
        {
            let mut file = create_file(&dir, &temp)?;
            file.truncate().map_err(error)?;
            Self::write_all(&mut file, data)?;
        }
        remove_in(&dir, name)?;
        rename_in(&dir, &temp, &dir, name)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.with_open(path, true, |o| {
            o.file.seek(SeekFrom::End(0)).map_err(error)?;
            o.dirty = true;
            o.file.write_all(data).map_err(error)
        })
    }

    fn sync(&self) -> Result<(), StorageError> {
        let mut result = Ok(());
        for o in self.open.borrow_mut().iter_mut() {
            if let Err(e) = o.flush() {
                result = Err(e);
            }
        }
        // What no file's flush reaches: folders and the FAT changed by
        // files written whole, created, renamed or removed.
        if let Some(flush) = &self.flush_disk
            && !flush()
        {
            result = Err(StorageError::Io);
        }
        result
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        if !dir.is_empty() {
            check(dir)?;
        }
        // The sizes in the folders, as appended.
        self.sync()?;
        for entry in entries(&self.dir(dir)?) {
            let entry = entry?;
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let is_dir = entry.is_dir();
            visit(DirEntry {
                name: &name,
                is_dir,
                size: if is_dir { 0 } else { entry.len() },
            });
        }
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        check(path)?;
        self.close(path)?;
        self.forget(path);
        match self.dir(parent(path)) {
            Ok(dir) => remove_in(&dir, name(path)),
            Err(StorageError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        check(from)?;
        check(to)?;
        if from == to {
            return Ok(());
        }
        // Nothing to move (a file or a folder): what is at `to` stays.
        if !self.is_there(from) {
            return Err(StorageError::NotFound);
        }
        self.close(from)?;
        self.close(to)?;
        self.forget(from);
        self.forget(to);
        self.create_dir(parent(to))?;
        let (dir, to_dir) = (self.dir(parent(from))?, self.dir(parent(to))?);
        // A file there is replaced.
        if lookup(&to_dir, name(to), false)?
            .entry
            .is_some_and(|entry| entry.is_file())
        {
            remove_in(&to_dir, name(to))?;
        }
        rename_in(&dir, name(from), &to_dir, name(to))
    }

    /// FAT names entries without regard to case.
    fn ignores_case(&self) -> bool {
        true
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(());
        }
        check(path)?;
        let mut dir = self.root();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            dir = create_dir_in(&dir, part)?;
        }
        Ok(())
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        check(path)?;
        self.close(path)?;
        self.forget(path);
        let dir = match self.dir(path) {
            Ok(dir) => dir,
            Err(StorageError::NotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        let mut names: alloc::vec::Vec<(String, bool)> = alloc::vec::Vec::new();
        for entry in entries(&dir) {
            let entry = entry?;
            let name = entry.file_name();
            if name != "." && name != ".." {
                names.push((name, entry.is_dir()));
            }
        }
        for (name, is_dir) in names {
            let child = format!("{path}/{name}");
            if is_dir {
                self.remove_dir_all(&child)?;
            } else {
                self.remove(&child)?;
            }
        }
        self.forget(path);
        remove_in(&self.dir(parent(path))?, name(path))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::boxed::Box;
    use alloc::vec;
    use alloc::vec::Vec;
    use fatfs::{FormatVolumeOptions, FsOptions, StdIoWrapper};
    use std::io::Cursor;

    type Disk = StdIoWrapper<Cursor<Vec<u8>>>;

    /// A fresh FAT disk of 4 MB, formatted.
    fn storage() -> FatStorage<Disk> {
        let mut disk = StdIoWrapper::from(Cursor::new(vec![0u8; 4 << 20]));
        fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
        disk.seek(SeekFrom::Start(0)).unwrap();
        let fs = FileSystem::new(disk, FsOptions::new()).unwrap();
        FatStorage::new(Box::leak(Box::new(fs)))
    }

    fn names(storage: &FatStorage<Disk>, dir: &str) -> Vec<(String, bool, u64)> {
        let mut out = Vec::new();
        storage
            .list(dir, &mut |e| {
                out.push((e.name.to_string(), e.is_dir, e.size))
            })
            .unwrap();
        out.sort();
        out
    }

    /// A card as blocks in memory, to look at as it is (after a cut).
    #[derive(Clone)]
    struct Cut(alloc::rc::Rc<RefCell<Vec<u8>>>);

    impl Blocks for Cut {
        fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
            let at = index as usize * BLOCK;
            block.copy_from_slice(&self.0.borrow()[at..at + BLOCK]);
            Ok(())
        }

        fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
            let at = index as usize * BLOCK;
            self.0.borrow_mut()[at..at + BLOCK].copy_from_slice(block);
            Ok(())
        }

        fn blocks(&mut self) -> Result<u32, DiskError> {
            Ok((self.0.borrow().len() / BLOCK) as u32)
        }
    }

    #[test]
    fn synced_is_on_the_card() {
        let mut formatted = StdIoWrapper::from(Cursor::new(vec![0u8; 4 << 20]));
        fatfs::format_volume(&mut formatted, FormatVolumeOptions::new()).unwrap();
        let card = Cut(alloc::rc::Rc::new(RefCell::new(
            formatted.into_inner().into_inner(),
        )));
        let disk = SharedDisk::new(BlockDisk::new(card.clone()).unwrap());
        let fs = FileSystem::new(disk.clone(), FsOptions::new()).unwrap();
        let s = FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk);
        // Emptied, then appended to in pieces (as files come over USB or
        // the network): the pieces stay in the open file until written
        // back.
        s.write("notes/a book.txt", b"").unwrap();
        s.append("notes/a book.txt", b"a high").unwrap();
        s.append("notes/a book.txt", b"light").unwrap();
        let before = card.0.borrow().clone();
        s.sync().unwrap();
        // Not on the card before the sync: cut off then, it would be lost.
        let fs =
            FileSystem::new(StdIoWrapper::from(Cursor::new(before)), FsOptions::new()).unwrap();
        let cut = FatStorage::new(Box::leak(Box::new(fs)));
        assert_ne!(cut.size("notes/a book.txt"), Ok(11));
        // The card as it is after it, as after a reset: the file is there.
        let bytes = card.0.borrow().clone();
        let fs = FileSystem::new(StdIoWrapper::from(Cursor::new(bytes)), FsOptions::new()).unwrap();
        let after = FatStorage::new(Box::leak(Box::new(fs)));
        let mut read = [0u8; 32];
        let n = after.read_at("notes/a book.txt", 0, &mut read).unwrap();
        assert_eq!(&read[..n], b"a highlight");
    }

    #[test]
    fn a_rename_of_nothing_keeps_what_is_there() {
        let s = storage();
        s.write("notes/kept.txt", b"highlights").unwrap();
        assert_eq!(
            s.rename("old/gone.txt", "notes/kept.txt"),
            Err(StorageError::NotFound)
        );
        assert_eq!(s.size("notes/kept.txt"), Ok(10));
    }

    #[test]
    fn long_names_written_read_and_listed() {
        let s = storage();
        let path = "Books/Lewis Carroll/Alice's Adventures in Wonderland.epub";
        s.write(path, b"the whole book").unwrap();
        assert_eq!(s.size(path), Ok(14));
        let mut buf = [0u8; 5];
        assert_eq!(s.read_at(path, 4, &mut buf), Ok(5));
        assert_eq!(&buf, b"whole");
        assert_eq!(s.read_at(path, 12, &mut buf), Ok(2));
        assert_eq!(&buf[..2], b"ok");
        assert_eq!(
            names(&s, "Books/Lewis Carroll"),
            [(
                String::from("Alice's Adventures in Wonderland.epub"),
                false,
                14
            )]
        );
        assert_eq!(names(&s, ""), [(String::from("Books"), true, 0)]);
        // Hidden folders with dots, as PaperPaw keeps its data.
        s.write(".paperpaw/plugins/reader/annotations.txt", b"a")
            .unwrap();
        assert!(s.exists(".paperpaw/plugins/reader/annotations.txt"));
        assert!(!s.exists("missing.txt"));
    }

    /// Folders kept from before stay right when folders are removed,
    /// made again, renamed, or more are used than are kept.
    #[test]
    fn kept_folders_follow_changes() {
        let s = storage();
        s.write("a/b/one.txt", b"1").unwrap();
        assert_eq!(s.size("a/b/one.txt"), Ok(1));
        // Removed and made again under the same name: another folder.
        s.remove_dir_all("a/b").unwrap();
        assert_eq!(s.size("a/b/one.txt"), Err(StorageError::NotFound));
        s.write("a/b/two.txt", b"22").unwrap();
        assert_eq!(s.size("a/b/two.txt"), Ok(2));
        assert_eq!(s.size("a/b/one.txt"), Err(StorageError::NotFound));
        // Renamed: found at the new path only.
        s.rename("a/b", "a/c").unwrap();
        assert_eq!(s.size("a/c/two.txt"), Ok(2));
        assert_eq!(s.size("a/b/two.txt"), Err(StorageError::NotFound));
        s.write("a/b/three.txt", b"333").unwrap();
        assert_eq!(names(&s, "a/b").len(), 1);
        // More folders than are kept, used by turns.
        for round in 0..2 {
            for i in 0..FOLDERS * 2 {
                let path = format!("many/f{i}/x.txt");
                if round == 0 {
                    s.write(&path, format!("{i}").as_bytes()).unwrap();
                }
                let mut buf = [0u8; 2];
                let n = s.read_at(&path, 0, &mut buf).unwrap();
                assert_eq!(&buf[..n], format!("{i}").as_bytes());
            }
        }
    }

    /// A disk of blocks in memory.
    struct Card {
        data: Vec<u8>,
    }

    impl Blocks for Card {
        fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
            let at = index as usize * BLOCK;
            block.copy_from_slice(self.data.get(at..at + BLOCK).ok_or(DiskError)?);
            Ok(())
        }
        fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
            let at = index as usize * BLOCK;
            self.data
                .get_mut(at..at + BLOCK)
                .ok_or(DiskError)?
                .copy_from_slice(block);
            Ok(())
        }
        fn blocks(&mut self) -> Result<u32, DiskError> {
            Ok((self.data.len() / BLOCK) as u32)
        }
    }

    #[test]
    fn whole_blocks_pass_by_the_kept_ones() {
        let mut disk = BlockDisk::new(Card {
            data: vec![0; 64 * BLOCK],
        })
        .unwrap();
        // Block 1 kept, changed, not yet written back.
        disk.seek(SeekFrom::Start(BLOCK as u64 + 10)).unwrap();
        disk.write_all(&[7; 100]).unwrap();
        // Four whole blocks over it: three pass by, one goes to its slot.
        let data: Vec<u8> = (0..4 * BLOCK).map(|i| (i % 251) as u8).collect();
        disk.seek(SeekFrom::Start(0)).unwrap();
        disk.write_all(&data).unwrap();
        let mut back = vec![0u8; 4 * BLOCK];
        disk.seek(SeekFrom::Start(0)).unwrap();
        disk.read_exact(&mut back).unwrap();
        assert_eq!(back, data);
        disk.flush().unwrap();
        assert_eq!(&disk.disk().data[..4 * BLOCK], &data[..]);
    }

    /// A disk of blocks in memory that counts its commands.
    struct Counted {
        data: Vec<u8>,
        commands: usize,
    }

    impl Blocks for Counted {
        fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
            self.read_blocks(index, block)
        }
        fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
            self.commands += 1;
            let at = index as usize * BLOCK;
            data.copy_from_slice(self.data.get(at..at + data.len()).ok_or(DiskError)?);
            Ok(())
        }
        fn write_block(&mut self, _: u32, _: &[u8; BLOCK]) -> Result<(), DiskError> {
            Err(DiskError)
        }
        fn blocks(&mut self) -> Result<u32, DiskError> {
            Ok((self.data.len() / BLOCK) as u32)
        }
    }

    #[test]
    fn blocks_read_one_after_the_other_are_read_ahead() {
        let data: Vec<u8> = (0..40 * BLOCK)
            .map(|i| (i / BLOCK * 7 + i % 13) as u8)
            .collect();
        let mut disk = BlockDisk::new(Counted {
            data: data.clone(),
            commands: 0,
        })
        .unwrap();
        // A folder's entries, 32 bytes at a time over 12 blocks: the first
        // block alone, then eight at once, then the rest of eight.
        let mut entry = [0u8; 32];
        for at in (0..12 * BLOCK).step_by(32) {
            disk.read_exact(&mut entry).unwrap();
            assert_eq!(&entry[..], &data[at..at + 32]);
        }
        assert_eq!(disk.disk().commands, 3);
        // Here and there: a block at a time.
        for block in [30u64, 20, 25] {
            disk.seek(SeekFrom::Start(block * BLOCK as u64)).unwrap();
            disk.read_exact(&mut entry).unwrap();
            assert_eq!(&entry[..], &data[block as usize * BLOCK..][..32]);
        }
        assert_eq!(disk.disk().commands, 6);
        // Not past the disk's end.
        disk.seek(SeekFrom::Start(37 * BLOCK as u64)).unwrap();
        let mut rest = vec![0u8; 3 * BLOCK];
        for chunk in rest.chunks_mut(100) {
            disk.read_exact(chunk).unwrap();
        }
        assert_eq!(&rest[..], &data[37 * BLOCK..]);
    }

    #[test]
    fn on_a_card_of_blocks() {
        let mut card = BlockDisk::new(Card {
            data: vec![0; 2 << 20],
        })
        .unwrap();
        fatfs::format_volume(&mut card, FormatVolumeOptions::new()).unwrap();
        card.flush().unwrap();
        card.seek(SeekFrom::Start(0)).unwrap();
        let fs = FileSystem::new(card, FsOptions::new()).unwrap();
        let s = FatStorage::new(Box::leak(Box::new(fs)));
        let text: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        s.write("Books/A long name.txt", &text).unwrap();
        let mut buf = [0u8; 700];
        assert_eq!(s.read_at("Books/A long name.txt", 1000, &mut buf), Ok(700));
        assert_eq!(&buf[..], &text[1000..1700]);
        assert_eq!(s.size("Books/A long name.txt"), Ok(3000));
    }

    /// A card of `blocks` blocks with a FAT volume formatted at block
    /// `first` and `table` written into its first blocks.
    fn card_with(blocks: usize, first: usize, table: &[(usize, &[u8])]) -> Card {
        let mut volume = StdIoWrapper::from(Cursor::new(vec![0u8; (blocks - first) * BLOCK]));
        fatfs::format_volume(&mut volume, FormatVolumeOptions::new()).unwrap();
        let mut data = vec![0u8; blocks * BLOCK];
        data[first * BLOCK..].copy_from_slice(&volume.into_inner().into_inner());
        for (at, bytes) in table {
            data[*at..*at + bytes.len()].copy_from_slice(bytes);
        }
        Card { data }
    }

    /// A partition table entry of an MBR (`kind` e.g. 0x0C, FAT32).
    fn mbr_entry(kind: u8, first: u32, count: u32) -> [u8; 16] {
        let mut entry = [0u8; 16];
        entry[4] = kind;
        entry[8..12].copy_from_slice(&first.to_le_bytes());
        entry[12..16].copy_from_slice(&count.to_le_bytes());
        entry
    }

    fn opens(card: Card) -> Result<(), VolumeError> {
        let disk = BlockDisk::volume(card)?;
        let fs = FileSystem::new(disk, FsOptions::new()).unwrap();
        let s = FatStorage::new(Box::leak(Box::new(fs)));
        s.write("Books/a.txt", b"found").unwrap();
        assert_eq!(s.size("Books/a.txt"), Ok(5));
        Ok(())
    }

    #[test]
    fn volumes_found_in_partitions() {
        let blocks = 8192;
        // The file system from the first block, without a table.
        assert_eq!(opens(card_with(blocks, 0, &[])), Ok(()));
        // An MBR, as on most cards, its partition after an empty one.
        let first = 2048;
        let entry = mbr_entry(0x0C, first as u32, (blocks - first) as u32);
        let table = [(446 + 16, &entry[..]), (510, &[0x55, 0xAA][..])];
        assert_eq!(opens(card_with(blocks, first, &table)), Ok(()));
        // Its table saying more blocks than the card has, as an X4 Pro's
        // card as sold.
        let entry = mbr_entry(0x0C, first as u32, (blocks - first + 512) as u32);
        let table = [(446, &entry[..]), (510, &[0x55, 0xAA][..])];
        assert_eq!(opens(card_with(blocks, first, &table)), Ok(()));
        // A GPT: a protective MBR, the header in block 1, entries in 2.
        let protective = mbr_entry(0xEE, 1, (blocks - 1) as u32);
        let mut header = [0u8; 92];
        header[..8].copy_from_slice(b"EFI PART");
        header[72..80].copy_from_slice(&2u64.to_le_bytes());
        header[80..84].copy_from_slice(&4u32.to_le_bytes());
        header[84..88].copy_from_slice(&128u32.to_le_bytes());
        let mut entry = [0u8; 48];
        entry[..16].copy_from_slice(&[0xA2; 16]);
        entry[32..40].copy_from_slice(&(first as u64).to_le_bytes());
        entry[40..48].copy_from_slice(&((blocks - 1) as u64).to_le_bytes());
        let table = [
            (446, &protective[..]),
            (510, &[0x55, 0xAA][..]),
            (BLOCK, &header[..]),
            (2 * BLOCK + 128, &entry[..]),
        ];
        assert_eq!(opens(card_with(blocks, first, &table)), Ok(()));
        // exFAT, as cards above 32 GB come.
        let mut card = card_with(
            blocks,
            first,
            &[
                (446, &mbr_entry(0x07, 2048, 6144)[..]),
                (510, &[0x55, 0xAA][..]),
            ],
        );
        card.data[first * BLOCK + 3..first * BLOCK + 11].copy_from_slice(b"EXFAT   ");
        assert_eq!(opens(card).unwrap_err(), VolumeError::ExFat);
        // Nothing at all.
        let card = Card {
            data: vec![0; blocks * BLOCK],
        };
        assert_eq!(opens(card).unwrap_err(), VolumeError::NoFat);
    }

    #[test]
    fn many_blocks_and_files_by_turns() {
        // More blocks than are kept, written and read by turns from more
        // files than stay open, on a card of blocks.
        let mut card = BlockDisk::new(Card {
            data: vec![0; 4 << 20],
        })
        .unwrap();
        fatfs::format_volume(&mut card, FormatVolumeOptions::new()).unwrap();
        card.flush().unwrap();
        card.seek(SeekFrom::Start(0)).unwrap();
        let fs = FileSystem::new(card, FsOptions::new()).unwrap();
        let s = FatStorage::new(Box::leak(Box::new(fs)));
        let content =
            |n: u32| -> Vec<u8> { (0..20_000u32).map(|i| ((i * 7 + n) % 251) as u8).collect() };
        let names: Vec<String> = (0..6).map(|n| alloc::format!("book/{n}.txt")).collect();
        for (n, name) in names.iter().enumerate() {
            s.write(name, &content(n as u32)).unwrap();
        }
        let mut buf = [0u8; 300];
        for round in 0..40u32 {
            let n = (round * 5 % 6) as usize;
            let at = u64::from(round * 487 % 19_000);
            assert_eq!(s.read_at(&names[n], at, &mut buf), Ok(300));
            let at = at as usize;
            assert_eq!(
                &buf[..],
                &content(n as u32)[at..at + 300],
                "{} at {at}",
                names[n]
            );
        }
        // Written again while open: read as written.
        s.write(&names[0], b"short").unwrap();
        assert_eq!(s.read_at(&names[0], 0, &mut buf), Ok(5));
    }

    #[test]
    fn appended_by_turns_and_written_back() {
        let s = storage();
        s.write("book.epub", &[7u8; 5000]).unwrap();
        let files: Vec<String> = (0..OPEN_FILES + 2)
            .map(|n| alloc::format!(".cache/{n}.bin"))
            .collect();
        let mut buf = [0u8; 100];
        // More files than stay open, appended to by turns, with a book
        // read between.
        for round in 0..30u8 {
            for (n, name) in files.iter().enumerate() {
                s.append(name, &[round, n as u8]).unwrap();
            }
            assert_eq!(
                s.read_at("book.epub", u64::from(round) * 100, &mut buf),
                Ok(100)
            );
        }
        for (n, name) in files.iter().enumerate() {
            assert_eq!(s.size(name), Ok(60));
            let mut all = [0u8; 60];
            assert_eq!(s.read_at(name, 0, &mut all), Ok(60));
            let want: Vec<u8> = (0..30u8).flat_map(|r| [r, n as u8]).collect();
            assert_eq!(&all[..], &want[..], "{name}");
        }
        // Listed with their sizes as appended.
        assert!(names(&s, ".cache").iter().all(|(_, _, size)| *size == 60));
        // Written back on sync: opened anew, a file has it all.
        s.append(&files[0], b"end").unwrap();
        s.sync().unwrap();
        let mut fresh = s.root().open_file(&files[0]).unwrap();
        assert_eq!(fresh.seek(SeekFrom::End(0)).unwrap(), 63);
        // And moved with it all.
        s.append(&files[1], b"end").unwrap();
        s.rename(&files[1], "moved.bin").unwrap();
        assert_eq!(s.size("moved.bin"), Ok(63));
        let mut fresh = s.root().open_file("moved.bin").unwrap();
        assert_eq!(fresh.seek(SeekFrom::End(0)).unwrap(), 63);
    }

    #[test]
    fn written_anew_appended_moved_and_removed() {
        let s = storage();
        s.write("a.txt", b"first, longer content").unwrap();
        // The file read last is open: a new content is seen anyway.
        let mut buf = [0u8; 32];
        assert_eq!(s.read_at("a.txt", 0, &mut buf), Ok(21));
        s.write("a.txt", b"second").unwrap();
        assert_eq!(s.read_at("a.txt", 0, &mut buf), Ok(6));
        assert_eq!(&buf[..6], b"second");
        s.append("a.txt", b" and more").unwrap();
        assert_eq!(s.size("a.txt"), Ok(15));
        assert_eq!(
            names(&s, ""),
            [(String::from("a.txt"), false, 15)],
            "no temporary left"
        );
        // Moved over another file, into a new folder.
        s.write("b/c.txt", b"old").unwrap();
        s.rename("a.txt", "b/c.txt").unwrap();
        assert!(!s.exists("a.txt"));
        assert_eq!(s.size("b/c.txt"), Ok(15));
        s.remove("b/c.txt").unwrap();
        s.remove("b/c.txt").unwrap();
        assert!(names(&s, "b").is_empty());
        // Folders with all in them.
        s.write("d/e/f.txt", b"x").unwrap();
        s.create_dir("d/g").unwrap();
        s.create_dir("d/g").unwrap();
        s.remove_dir_all("d").unwrap();
        s.remove_dir_all("d").unwrap();
        assert_eq!(names(&s, ""), [(String::from("b"), true, 0)]);
        assert_eq!(
            s.read_at("../x", 0, &mut buf),
            Err(StorageError::InvalidPath)
        );
    }

    /// The short name FAT keeps beside `long` in folder `dir`.
    fn short_name(s: &FatStorage<Disk>, dir: &str, long: &str) -> String {
        s.dir(dir)
            .unwrap()
            .iter()
            .map(Result::unwrap)
            .find(|entry| entry.file_name() == long)
            .unwrap()
            .short_file_name()
    }

    /// A short name stands for nothing where its entry has a long name:
    /// `PAPERP~1` does not reach PaperPaw's data, by any call.
    #[test]
    fn short_names_of_long_ones_reach_nothing() {
        let s = storage();
        s.write(".paperpaw/secrets.txt", b"password").unwrap();
        s.write(".paperpaw/plugins/wifi/networks.txt", b"home")
            .unwrap();
        s.write("Books/A long title.epub", b"a book").unwrap();
        assert_eq!(short_name(&s, "", ".paperpaw"), "PAPERP~1");
        assert_eq!(short_name(&s, "Books", "A long title.epub"), "ALONGT~1.EPU");
        let alias = "PAPERP~1/secrets.txt";
        let mut buf = [0u8; 16];
        // Not read, nor found at all.
        assert_eq!(s.read_at(alias, 0, &mut buf), Err(StorageError::NotFound));
        assert_eq!(s.size(alias), Err(StorageError::NotFound));
        assert!(!s.exists("PAPERP~1"));
        assert!(!s.exists("paperp~1/secrets.txt"));
        assert_eq!(s.list("PAPERP~1", &mut |_| {}), Err(StorageError::NotFound));
        assert_eq!(
            s.list("PAPERP~1/plugins", &mut |_| {}),
            Err(StorageError::NotFound)
        );
        assert_eq!(s.size("Books/ALONGT~1.EPU"), Err(StorageError::NotFound));
        // Not written, appended to, made, moved or removed through it.
        assert_eq!(s.write(alias, b"stolen"), Err(StorageError::InvalidPath));
        assert_eq!(
            s.append("PAPERP~1/firmware-keys.txt", b"key"),
            Err(StorageError::InvalidPath)
        );
        assert_eq!(
            s.create_dir("PAPERP~1/plugins"),
            Err(StorageError::InvalidPath)
        );
        assert_eq!(s.write("PAPERP~1", b"x"), Err(StorageError::InvalidPath));
        assert_eq!(s.create_dir("PAPERP~1"), Err(StorageError::InvalidPath));
        assert_eq!(s.rename(alias, "stolen.txt"), Err(StorageError::NotFound));
        assert_eq!(s.rename("PAPERP~1", "stolen"), Err(StorageError::NotFound));
        assert_eq!(
            s.rename("Books/A long title.epub", "PAPERP~1/a.epub"),
            Err(StorageError::InvalidPath)
        );
        assert_eq!(
            s.rename("Books/A long title.epub", "PAPERP~1"),
            Err(StorageError::InvalidPath)
        );
        s.remove(alias).unwrap();
        s.remove("PAPERP~1").unwrap();
        s.remove_dir_all("PAPERP~1").unwrap();
        s.remove_dir_all("PAPERP~1/plugins").unwrap();
        // All as it was, nothing left beside.
        assert_eq!(s.read_at(".paperpaw/secrets.txt", 0, &mut buf), Ok(8));
        assert_eq!(&buf[..8], b"password");
        assert_eq!(s.size(".paperpaw/plugins/wifi/networks.txt"), Ok(4));
        assert_eq!(
            names(&s, ".paperpaw"),
            [
                (String::from("plugins"), true, 0),
                (String::from("secrets.txt"), false, 8)
            ]
        );
        assert_eq!(
            names(&s, ""),
            [
                (String::from(".paperpaw"), true, 0),
                (String::from("Books"), true, 0)
            ]
        );
        assert_eq!(s.size("Books/A long title.epub"), Ok(6));
        // A long name reaches its entry, without regard to case.
        assert_eq!(s.size(".PaperPaw/SECRETS.TXT"), Ok(8));
        assert_eq!(s.size("books/a LONG title.epub"), Ok(6));
    }

    /// A card of 4 MB formatted with `options`, `files` written on it.
    fn card_with_files(options: FormatVolumeOptions, files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut formatted = StdIoWrapper::from(Cursor::new(vec![0u8; 4 << 20]));
        fatfs::format_volume(&mut formatted, options).unwrap();
        let card = Cut(alloc::rc::Rc::new(RefCell::new(
            formatted.into_inner().into_inner(),
        )));
        let disk = SharedDisk::new(BlockDisk::new(card.clone()).unwrap());
        let fs = FileSystem::new(disk.clone(), FsOptions::new()).unwrap();
        let s = FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk);
        for (path, data) in files {
            s.write(path, data).unwrap();
        }
        s.sync().unwrap();
        card.0.borrow().clone()
    }

    /// The storage on the card `bytes`.
    fn mounted(bytes: Vec<u8>) -> FatStorage<Disk> {
        let fs = FileSystem::new(StdIoWrapper::from(Cursor::new(bytes)), FsOptions::new()).unwrap();
        FatStorage::new(Box::leak(Box::new(fs)))
    }

    /// A card with `files` written, then their long names taken away, as
    /// systems write names that fit in 8.3.
    fn short_names_only(files: &[(&str, &[u8])]) -> FatStorage<Disk> {
        let mut bytes = card_with_files(FormatVolumeOptions::new(), files);
        // Every long name entry (attributes 0x0F, type 0, no cluster)
        // marked removed.
        for entry in bytes.as_chunks_mut::<32>().0 {
            if entry[11] == 0x0F && entry[12] == 0 && entry[26..28] == [0, 0] && entry[0] != 0 {
                entry[0] = 0xE5;
            }
        }
        mounted(bytes)
    }

    #[test]
    fn folders_end_after_the_most_entries_fat_holds() {
        // Endless, as the entries of a folder whose clusters run in a loop.
        let endless = core::iter::repeat_with(|| Ok(()));
        assert_eq!(bounded(endless).position(|r| r.is_err()), Some(ENTRIES));
        let few = [Ok(1), Ok(2)].into_iter();
        assert_eq!(bounded(few).collect::<Vec<_>>(), [Ok(1), Ok(2)]);
    }

    /// A damaged card, a folder's clusters in a loop: reading the folder
    /// fails instead of going on for ever.
    #[test]
    fn folders_in_a_loop_end() {
        // Clusters of one block: 16 entries. `.`, `..` and seven files
        // of two entries each (a long name and a short one) fill the
        // first; the eighth file takes another.
        let files: Vec<(String, &[u8])> = (0..8)
            .map(|n| (alloc::format!("Loop/f{n}.txt"), &b"x"[..]))
            .collect();
        let files: Vec<(&str, &[u8])> = files.iter().map(|(p, d)| (p.as_str(), *d)).collect();
        let options = FormatVolumeOptions::new()
            .fat_type(fatfs::FatType::Fat16)
            .bytes_per_cluster(BLOCK as u32);
        let mut bytes = card_with_files(options, &files);
        // The folder's first cluster, from its short entry in the root.
        let entry = bytes
            .as_chunks::<32>()
            .0
            .iter()
            .find(|e| &e[..11] == b"LOOP       " && e[11] & 0x10 != 0)
            .unwrap();
        let cluster = usize::from(u16::from_le_bytes([entry[26], entry[27]]));
        // Its next cluster in both FATs: itself.
        let reserved = usize::from(u16::from_le_bytes([bytes[14], bytes[15]]));
        let per_fat = usize::from(u16::from_le_bytes([bytes[22], bytes[23]]));
        for fat in 0..usize::from(bytes[16]) {
            let at = (reserved + fat * per_fat) * BLOCK + cluster * 2;
            bytes[at..at + 2].copy_from_slice(&(cluster as u16).to_le_bytes());
        }
        let s = mounted(bytes);
        assert_eq!(s.size("Loop/missing.txt"), Err(StorageError::Io));
        assert_eq!(s.list("Loop", &mut |_| {}), Err(StorageError::Io));
        assert_eq!(s.write("Loop/new.txt", b"new"), Err(StorageError::Io));
        assert_eq!(s.remove_dir_all("Loop"), Err(StorageError::Io));
    }

    /// An entry with a short name alone is found by it.
    #[test]
    fn short_names_alone_are_names() {
        let s = short_names_only(&[("BOOKS/README.TXT", b"read me")]);
        assert!(
            s.dir("BOOKS")
                .unwrap()
                .iter()
                .map(Result::unwrap)
                .all(|entry| entry.long_file_name_as_ucs2_units().is_none())
        );
        assert_eq!(names(&s, ""), [(String::from("BOOKS"), true, 0)]);
        assert_eq!(s.size("BOOKS/README.TXT"), Ok(7));
        assert_eq!(s.size("books/readme.txt"), Ok(7));
        s.write("books/readme.txt", b"again").unwrap();
        s.rename("Books/ReadMe.txt", "books/notes.txt").unwrap();
        assert_eq!(names(&s, "BOOKS"), [(String::from("notes.txt"), false, 5)]);
        s.remove("BOOKS/NOTES.TXT").unwrap();
        assert!(names(&s, "books").is_empty());
    }
}
