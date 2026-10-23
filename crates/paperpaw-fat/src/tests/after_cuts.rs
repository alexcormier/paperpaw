//! Moves, listings, appends and leftovers after a cut at any point.

use super::*;

/// A file moved, cut off at any point, is whole at its old place or
/// its new one, or at both until the next change settles it: to a
/// free name in its folder, in another or in a new one, however full
/// its folder. Settled, no two files share a cluster and none is lost
/// (but that of a folder made for it, cut off while it is made).
#[test]
fn a_file_moved_to_another_folder_is_found_after_any_cut() {
    let (old, _) = old_and_new();
    for (to, crowded) in [
        ("Books/c.txt", true),
        ("Other/c.txt", false),
        ("Other/c.txt", true),
        ("Moved/c.txt", false),
    ] {
        let image = prepared(|s| {
            beside(s, crowded);
            s.write(A, &old).unwrap();
            // A move before: the card holds its record of moves (made
            // once, as a file is written).
            s.write("Other/d.txt", b"d").unwrap();
            s.rename("Other/d.txt", "Other/e.txt").unwrap();
        });
        let cuts = cut_images(image, |s| s.rename(A, to).unwrap());
        let last = cuts.len() - 1;
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            let found = [whole(&s, A).ok(), whole(&s, to).ok()];
            assert!(found.contains(&Some(old.clone())), "{to} {n}");
            assert!(found.iter().flatten().all(|f| *f == old), "{to} {n}");
            assert_beside(&s, crowded, n);
            // The next change settles it.
            s.write("other.txt", b"other").unwrap();
            let found = [whole(&s, A).ok(), whole(&s, to).ok()];
            match &found {
                [Some(a), None] | [None, Some(a)] if *a == old => {}
                _ => panic!("{to}, cut after {n}: {:?}", found.map(|f| f.is_some())),
            }
            if n == last {
                assert_eq!(found[1].as_ref(), Some(&old));
            }
            assert_beside(&s, crowded, n);
            let made = if to.starts_with("Moved") { 1 } else { 0 };
            assert_nothing_lost(&s, made, n);
        }
    }
}

/// A folder moved, cut off at any point, is found with all in it at its
/// old place or its new one once the next change settles it: to another
/// folder, or to another case of its name.
#[test]
fn a_folder_moved_is_found_after_any_cut() {
    for to in ["Other/Sub", "Books/SUB"] {
        let image = prepared(|s| {
            beside(s, true);
            s.write("Books/Sub/x.txt", b"x").unwrap();
            s.write("Other/d.txt", b"d").unwrap();
            s.rename("Other/d.txt", "Other/e.txt").unwrap();
        });
        let cuts = cut_images(image, |s| s.rename("Books/Sub", to).unwrap());
        let last = cuts.len() - 1;
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            s.write("other.txt", b"other").unwrap();
            let folders: Vec<_> = ["Books", "Other"]
                .into_iter()
                .flat_map(|dir| names(&s, dir).into_iter().map(move |e| (dir, e)))
                .filter(|(_, (name, is_dir, _))| *is_dir && name.eq_ignore_ascii_case("sub"))
                .map(|(dir, (name, ..))| format!("{dir}/{name}"))
                .collect();
            assert_eq!(folders.len(), 1, "{to} {n}: {folders:?}");
            let x = whole(&s, &format!("{}/x.txt", folders[0]));
            assert_eq!(x.as_deref(), Ok(&b"x"[..]), "{to} {n}");
            if n == last {
                assert_eq!(folders[0], to);
            }
            assert_beside(&s, true, n);
            assert_nothing_lost(&s, 0, n);
        }
    }
}

/// A move on a full card, where the folder it goes to must grow, fails
/// and keeps the file where it was.
#[test]
fn a_move_on_a_full_card_keeps_the_file() {
    let s = mounted(prepared(|s| {
        s.write("Books/a.txt", b"a book").unwrap();
        // Moves before: the record of moves is there.
        s.rename("Books/a.txt", "Books/b.txt").unwrap();
        s.rename("Books/b.txt", "Books/a.txt").unwrap();
    }));
    // The card full, then a folder whose entries fill its cluster.
    s.create_dir("Full").unwrap();
    while s.append("big.bin", &[7u8; 64 * 1024]).is_ok() {}
    let mut n = 0;
    while s.write(&format!("Full/f{n:02}.txt"), b"").is_ok() {
        n += 1;
    }
    assert!(n > 10, "{n}");
    assert_eq!(
        s.rename("Books/a.txt", "Full/a.txt"),
        Err(StorageError::Full)
    );
    assert_eq!(whole(&s, "Books/a.txt").as_deref(), Ok(&b"a book"[..]));
    assert!(!s.exists("Full/a.txt"));
}

/// The files a write goes through are not listed, nor taken by name:
/// one may hold the clusters of the file itself (cut off in a write).
#[test]
fn temporary_files_are_not_listed_nor_removed_by_name() {
    let (old, new) = old_and_new();
    let image = prepared(|s| {
        beside(s, true);
        s.write(A, &old).unwrap();
    });
    let temps = Temps::of(name(A));
    let cuts = cut_images(image, |s| s.write(A, &new).unwrap());
    for (n, image) in cuts.into_iter().enumerate() {
        let s = mounted(image);
        let listed = names(&s, "Books");
        assert!(
            listed.iter().all(|(name, ..)| !name.starts_with('~')),
            "{n}"
        );
        for temp in [temps.tmp(), temps.ready()] {
            let path = format!("Books/{temp}");
            assert_eq!(s.remove(&path), Err(StorageError::InvalidPath), "{n}");
        }
        s.write("Books/other.bin", &[9u8; 8000]).unwrap();
        let a = whole(&s, A);
        assert!(a == Ok(old.clone()) || a == Ok(new.clone()), "{n}");
        assert_eq!(cross_linked(&s), Vec::<String>::new(), "{n}");
    }
}

/// A file written anew, cut off at any point, is listed in its folder,
/// as a library's scan finds files, before it is looked up by name.
#[test]
fn a_file_written_anew_is_listed_after_any_cut() {
    let (old, new) = old_and_new();
    for crowded in [false, true] {
        let image = prepared(|s| {
            beside(s, crowded);
            s.write(A, &old).unwrap();
        });
        let cuts = cut_images(image, |s| s.write(A, &new).unwrap());
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            let sizes = [old.len() as u64, new.len() as u64];
            let listed = names(&s, "Books")
                .into_iter()
                .filter(|(file, _, size)| file == name(A) && sizes.contains(size))
                .count();
            assert_eq!(listed, 1, "{crowded} {n}");
        }
    }
}

/// A file appended to after a cut, before anything else is written,
/// keeps what was appended: its new content, whole beside it, takes its
/// place before it is opened, not after.
#[test]
fn a_file_appended_to_after_a_cut_keeps_what_was_appended() {
    let (old, new) = old_and_new();
    for crowded in [false, true] {
        let image = prepared(|s| {
            beside(s, crowded);
            s.write(A, &old).unwrap();
        });
        let cuts = cut_images(image, |s| s.write(A, &new).unwrap());
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            s.append(A, b" more").unwrap();
            let appended = whole(&s, A).unwrap();
            let after = |data: &[u8]| [data, b" more"].concat();
            assert!(appended == after(&old) || appended == after(&new), "{n}");
            s.rename(A, "Books/moved.txt").unwrap();
            assert_eq!(whole(&s, "Books/moved.txt"), Ok(appended), "{crowded} {n}");
        }
    }
}

/// A file changed elsewhere after a cut (on a computer: a newer time)
/// is kept: its new content beside it, older, is let go.
#[test]
fn a_file_changed_elsewhere_after_a_cut_is_kept() {
    let (old, new) = old_and_new();
    let image = prepared(|s| {
        beside(s, false);
        s.write(A, &old).unwrap();
    });
    let cuts = cut_images(image, |s| s.write(A, &new).unwrap());
    for (n, image) in cuts.into_iter().enumerate() {
        let s = mounted(image);
        let books = s.fs.root_dir().open_dir("Books").unwrap();
        // Not where the old file and the new one are both named so (cut
        // off within the replace's own entries): the new one is kept.
        let named = books
            .iter()
            .filter(|e| e.as_ref().unwrap().file_name() == name(A));
        if named.count() != 1 {
            continue;
        }
        let mut file = books.open_file(name(A)).unwrap();
        file.truncate().unwrap();
        file.write_all(b"edited").unwrap();
        let date = fatfs::Date::new(2026, 10, 23);
        file.set_modified(fatfs::DateTime::new(date, fatfs::Time::new(12, 0, 0, 0)));
        file.flush().unwrap();
        drop(file);
        s.rename(A, "Books/moved.txt").unwrap();
        assert_eq!(
            whole(&s, "Books/moved.txt").as_deref(),
            Ok(&b"edited"[..]),
            "{n}"
        );
    }
}

/// Temporary files of an earlier version (`~pp<n>.tmp`) are taken away
/// at the next write in their folder where they are no file's only
/// copy: empty, or the same as a file beside them. Others stay, and
/// writes go on beside them.
#[test]
fn old_temporaries_are_taken_away_where_superseded() {
    let (old, new) = old_and_new();
    let s = mounted(prepared(|s| {
        s.write(A, &old).unwrap();
        s.write("Books/b.txt", b"beside").unwrap();
    }));
    write_raw(&s, "Books", "~pp0.tmp", b"");
    write_raw(&s, "Books", "~pp1.tmp", &old);
    write_raw(&s, "Books", "~pp2.tmp", &new);
    write_raw(&s, "Books", "~pp3.tmp", b"beside");
    write_raw(&s, "Books", "~pp4.tmp", b"besidf");
    s.dir("Books").unwrap().create_dir("~pp5.tmp").unwrap();
    write_raw(&s, "Books", "~pp63.tmp", b"");
    write_raw(&s, "Books", "~pp6.txt", b"");
    write_raw(&s, "", "~pp0.tmp", b"");
    s.write("Books/c.txt", b"c").unwrap();
    let left: Vec<_> = names(&s, "Books")
        .into_iter()
        .map(|(name, ..)| name)
        .collect();
    let kept = ["A rather long name.txt", "b.txt", "c.txt"];
    let kept = kept
        .iter()
        .chain(&["~pp2.tmp", "~pp4.tmp", "~pp5.tmp", "~pp6.txt"]);
    assert_eq!(left, kept.copied().collect::<Vec<_>>());
    assert_eq!(whole(&s, "Books/~pp2.tmp"), Ok(new.clone()));
    assert_eq!(whole(&s, A), Ok(old.clone()));
    assert_eq!(whole(&s, "Books/b.txt").as_deref(), Ok(&b"beside"[..]));
    // Only where written.
    assert!(s.exists("~pp0.tmp"));
    assert_eq!(cross_linked(&s), Vec::<String>::new());
}

/// A folder removed after a write cut off at any point frees each
/// cluster once: a file and its new content may share them.
#[test]
fn a_folder_removed_after_a_cut_frees_each_cluster_once() {
    let (old, new) = old_and_new();
    for crowded in [false, true] {
        let image = prepared(|s| {
            beside(s, crowded);
            s.write(A, &old).unwrap();
        });
        let cuts = cut_images(image, |s| s.write(A, &new).unwrap());
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            let before = free_clusters(&s);
            s.remove_dir_all("Books").unwrap();
            assert!(free_clusters(&s) > before, "{n}");
            s.write("one.bin", &[1u8; 9000]).unwrap();
            s.write("two.bin", &[2u8; 9000]).unwrap();
            assert_eq!(whole(&s, "one.bin"), Ok(vec![1u8; 9000]), "{n}");
            assert_eq!(cross_linked(&s), Vec::<String>::new(), "{n}");
        }
    }
}

/// An empty file written over another, cut off at any point, leaves
/// its folder usable and the file with its old content or none, listed
/// once: an empty file holds no cluster that tells its two entries
/// apart.
#[test]
fn an_empty_file_written_over_another_is_found_after_any_cut() {
    let (old, _) = old_and_new();
    for before in [&old[..], b""] {
        let image = prepared(|s| {
            beside(s, false);
            s.write(A, before).unwrap();
        });
        let cuts = cut_images(image, |s| s.write(A, b"").unwrap());
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            let a = whole(&s, A);
            assert!(a == Ok(before.to_vec()) || a == Ok(Vec::new()), "{n}");
            assert_beside(&s, false, n);
            s.write("Books/other.txt", b"other").unwrap();
            let a = whole(&s, A);
            assert!(a == Ok(before.to_vec()) || a == Ok(Vec::new()), "{n}");
            let listed = names(&s, "Books")
                .into_iter()
                .filter(|(file, ..)| file == name(A))
                .count();
            assert_eq!(listed, 1, "{n}");
            assert_eq!(cross_linked(&s), Vec::<String>::new(), "{n}");
        }
    }
}

/// An empty file moved over another, cut off at any point, leaves the
/// card usable: the other is kept or replaced, the empty one at one
/// place once the next change settles it.
#[test]
fn an_empty_file_moved_over_another_is_found_after_any_cut() {
    let (old, _) = old_and_new();
    let from = "Books/e.part";
    let image = prepared(|s| {
        beside(s, false);
        s.write(A, &old).unwrap();
        s.write(from, b"").unwrap();
        // A move before: the card holds its record of moves.
        s.write("Inbox/d.txt", b"d").unwrap();
        s.rename("Inbox/d.txt", "Inbox/e.txt").unwrap();
    });
    let cuts = cut_images(image, |s| s.rename(from, A).unwrap());
    let last = cuts.len() - 1;
    for (n, image) in cuts.into_iter().enumerate() {
        let s = mounted(image);
        s.write("other.txt", b"other").unwrap();
        let found = (whole(&s, A), whole(&s, from).ok());
        let settled =
            found == (Ok(Vec::new()), None) || found == (Ok(old.clone()), Some(Vec::new()));
        assert!(settled, "{n}: {:?}", (found.0.map(|a| a.len()), found.1));
        if n == last {
            assert_eq!(found, (Ok(Vec::new()), None));
        }
        let listed = names(&s, "Books")
            .into_iter()
            .filter(|(file, ..)| file == name(A))
            .count();
        assert_eq!(listed, 1, "{n}");
        assert_beside(&s, false, n);
        assert_eq!(cross_linked(&s), Vec::<String>::new(), "{n}");
    }
}

/// A card whose folder `BOOKS` cannot be settled: three entries named
/// `A.TXT` (as a damaged card may hold them) and new content of it
/// beside them. A move to `BOOKS/A.TXT` recorded, where `journal`.
fn unsettleable(journal: bool) -> FatStorage<Disk> {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    for (path, data) in [
        ("BOOKS/A.TXT", &b"a"[..]),
        ("BOOKS/B.TXT", b"b"),
        ("BOOKS/C.TXT", b"c"),
        ("BOOKS/D.TXT", b"d"),
        ("NOTES.TXT", b"notes"),
    ] {
        s.write(path, data).unwrap();
    }
    let temps = Temps::of("A.TXT");
    let mut file = s.dir("BOOKS").unwrap().create_file(temps.ready()).unwrap();
    file.write_all(b"new").unwrap();
    file.flush().unwrap();
    drop(file);
    if journal {
        let record = settle::move_record("INBOX/A.TXT", "BOOKS/A.TXT").unwrap();
        s.write_move(&record).unwrap();
    }
    s.sync().unwrap();
    let mut bytes = card.image();
    for entry in bytes.as_chunks_mut::<32>().0 {
        // Long names taken away (see `short_names_only`).
        if entry[11] == 0x0F && entry[12] == 0 && entry[26..28] == [0, 0] && entry[0] != 0 {
            entry[0] = 0xE5;
        }
        if entry[..11] == *b"B       TXT" || entry[..11] == *b"C       TXT" {
            entry[..11].copy_from_slice(b"A       TXT");
        }
    }
    mounted(bytes)
}

/// A folder that cannot be settled stays readable and writable: its
/// files as they are, each whole.
#[test]
fn a_folder_that_cannot_be_settled_stays_usable() {
    let s = unsettleable(false);
    let listed: Vec<_> = names(&s, "BOOKS").into_iter().map(|(n, ..)| n).collect();
    assert_eq!(listed, ["A.TXT", "A.TXT", "A.TXT", "D.TXT"]);
    assert_eq!(whole(&s, "BOOKS/D.TXT").as_deref(), Ok(&b"d"[..]));
    s.write("BOOKS/E.TXT", b"e").unwrap();
    assert_eq!(whole(&s, "BOOKS/E.TXT").as_deref(), Ok(&b"e"[..]));
    assert_eq!(whole(&s, "NOTES.TXT").as_deref(), Ok(&b"notes"[..]));
}

/// A move that cannot be settled holds up changes at its own paths
/// only, and other moves (which would be recorded over it): all else
/// is read and written.
#[test]
fn a_move_that_cannot_be_settled_holds_up_its_paths_only() {
    let s = unsettleable(true);
    assert_eq!(whole(&s, "NOTES.TXT").as_deref(), Ok(&b"notes"[..]));
    assert_eq!(whole(&s, "BOOKS/D.TXT").as_deref(), Ok(&b"d"[..]));
    assert_eq!(names(&s, "BOOKS").len(), 4);
    s.write("NOTES.TXT", b"more notes").unwrap();
    s.append("BOOKS/D.TXT", b"d").unwrap();
    s.remove("BOOKS/E.TXT").unwrap();
    assert_eq!(whole(&s, "NOTES.TXT").as_deref(), Ok(&b"more notes"[..]));
    assert_eq!(whole(&s, "BOOKS/D.TXT").as_deref(), Ok(&b"dd"[..]));
    assert!(s.write("BOOKS/A.TXT", b"x").is_err());
    assert!(s.remove("BOOKS/A.TXT").is_err());
    assert!(s.remove_dir_all("BOOKS").is_err());
    assert!(s.rename("NOTES.TXT", "OTHER.TXT").is_err());
    assert!(s.size("BOOKS/A.TXT").is_err());
    // Still recorded: tried again at the next change.
    assert!(s.recorded().unwrap().is_some());
}

/// The card `image` after `op`, cut off once the new content of file
/// `A` is whole beside it (named `.NEW`), before it takes its place.
fn new_content_beside(image: Vec<u8>, op: impl FnOnce(&FatStorage<SharedDisk<Cut>>)) -> Vec<u8> {
    let temps = Temps::of(name(A));
    cut_images(image, op)
        .into_iter()
        .find(|image| {
            let s = mounted(image.clone());
            let books = s.fs.root_dir().open_dir("Books").unwrap();
            let entries: Vec<_> = books.iter().map(Result::unwrap).collect();
            let has = |n: &str| entries.iter().any(|e| e.short_file_name() == n);
            let named = entries.iter().filter(|e| e.file_name() == name(A)).count();
            has(temps.ready()) && !has(temps.tmp()) && named == 1
        })
        .unwrap()
}

/// A file stamped by a computer (any time is newer than the X4's),
/// written by PaperPaw, cut off once its new content is whole beside
/// it: the new content takes its place.
#[test]
fn a_file_stamped_elsewhere_takes_its_new_content_after_a_cut() {
    let (old, new) = old_and_new();
    let image = prepared(|s| {
        beside(s, false);
        s.write(A, &old).unwrap();
        let mut file = s.dir("Books").unwrap().open_file(name(A)).unwrap();
        let date = fatfs::Date::new(2025, 5, 6);
        file.set_modified(fatfs::DateTime::new(date, fatfs::Time::new(8, 0, 0, 0)));
        file.flush().unwrap();
    });
    let s = mounted(new_content_beside(image, |s| s.write(A, &new).unwrap()));
    assert_eq!(whole(&s, A), Ok(new));
}

/// A file appended to after its new content was made beside it (past
/// the storage; the X4 stamps no time): it keeps what was appended.
#[test]
fn a_file_appended_to_after_its_new_content_was_made_is_kept() {
    let (old, new) = old_and_new();
    let image = prepared(|s| {
        beside(s, false);
        s.write(A, &old).unwrap();
    });
    let image = new_content_beside(image, |s| s.write(A, &new).unwrap());
    let disk = StdIoWrapper::from(Cursor::new(image));
    let fs: &'static _ = Box::leak(Box::new(FileSystem::new(disk, FsOptions::new()).unwrap()));
    {
        let books = fs.root_dir().open_dir("Books").unwrap();
        let mut file = books.open_file(name(A)).unwrap();
        file.seek(SeekFrom::End(0)).unwrap();
        file.write_all(b" more").unwrap();
        file.flush().unwrap();
    }
    let s = FatStorage::new(fs);
    assert_eq!(whole(&s, A), Ok([&old[..], b" more"].concat()));
}
