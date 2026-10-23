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
