//! The check that crates keep no mutable statics (CI runs it on the
//! SDK's): a native plugin carries its own copy of the SDK code it uses,
//! and has no writable data, so a static that changes would leave it
//! unable to load (ADR 0024).
//!
//! It reads rlibs (and other `ar` archives, GNU and System V, with the
//! `/` symbol table and the `//` table of long names) and, in every ELF
//! object in them, finds the symbols defined in writable memory
//! (`.data*`, `.bss*`, `.sdata*`, `.sbss*` and any other section with
//! `SHF_WRITE` and `SHF_ALLOC`). Other members (`lib.rmeta`) are passed
//! over; LLVM bitcode (crates built for LTO) is refused, as it would
//! hide what it holds.

use crate::elf::{self, Elf, SHN_COMMON, SHN_LORESERVE, STT_SECTION};

const MAGIC: &[u8] = b"!<arch>\n";
const MEMBER_HEAD_LEN: usize = 60;

/// A member of an archive.
#[derive(Debug, PartialEq, Eq)]
pub struct Member<'a> {
    pub name: String,
    pub data: &'a [u8],
}

/// The members of the archive `bytes`, in order, without its tables.
pub fn members(bytes: &[u8]) -> Result<Vec<Member<'_>>, String> {
    if !bytes.starts_with(MAGIC) {
        return Err("not an ar archive".into());
    }
    let mut members = Vec::new();
    let mut long_names: &[u8] = &[];
    let mut at = MAGIC.len();
    while at < bytes.len() {
        let head = bytes
            .get(at..at + MEMBER_HEAD_LEN)
            .ok_or("the archive is cut short")?;
        if &head[58..60] != b"`\n" {
            return Err(format!("no member's header at {at}"));
        }
        let size: usize = std::str::from_utf8(&head[48..58])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| format!("a member's size at {at} is no number"))?;
        let start = at + MEMBER_HEAD_LEN;
        let data = start
            .checked_add(size)
            .and_then(|end| bytes.get(start..end))
            .ok_or("the archive is cut short")?;
        at = start + size + size % 2;
        let name = String::from_utf8_lossy(&head[..16]);
        let name = name.trim_end();
        match name {
            "/" | "/SYM64/" => continue,
            "//" => {
                long_names = data;
                continue;
            }
            _ => {}
        }
        let name = if let Some(offset) = name.strip_prefix('/') {
            let offset: usize = offset
                .parse()
                .map_err(|_| format!("a member's name `{name}` is not read"))?;
            let rest = long_names
                .get(offset..)
                .ok_or_else(|| format!("a member's name `{name}` lies outside the names"))?;
            let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
            String::from_utf8_lossy(&rest[..end])
                .trim_end_matches('/')
                .to_string()
        } else if name.starts_with("#1/") {
            return Err("BSD archives are not read".into());
        } else {
            name.trim_end_matches('/').to_string()
        };
        members.push(Member { name, data });
    }
    Ok(members)
}

/// A symbol defined in writable memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Static {
    /// The object it is in.
    pub object: String,
    pub symbol: String,
    /// Its section (`COMMON` for common symbols).
    pub section: String,
    pub size: u32,
}

/// Whether `bytes` are LLVM bitcode, plain or wrapped.
fn is_bitcode(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BC\xC0\xDE") || bytes.starts_with(b"\xDE\xC0\x17\x0B")
}

/// The mutable statics of the archive `bytes`, which must hold objects
/// (not bitcode only, as built for LTO), lest the check pass unseen.
pub fn statics(bytes: &[u8]) -> Result<Vec<Static>, String> {
    let mut found = Vec::new();
    let mut objects = 0;
    for member in members(bytes)? {
        if is_bitcode(member.data) {
            return Err(format!(
                "{}: LLVM bitcode, not an object: build the crates without LTO for this check",
                member.name
            ));
        }
        if !elf::is_elf(member.data) {
            continue;
        }
        objects += 1;
        let elf = Elf::parse(member.data).map_err(|e| format!("{}: {e}", member.name))?;
        let Some((_, symbols)) = elf.symbols().map_err(|e| format!("{}: {e}", member.name))? else {
            continue;
        };
        for symbol in symbols {
            if symbol.size == 0 || symbol.kind() == STT_SECTION {
                continue;
            }
            let section = match symbol.shndx {
                SHN_COMMON => "COMMON".to_string(),
                i if i < SHN_LORESERVE => match elf.sections.get(usize::from(i)) {
                    Some(s) if s.is_writable() => s.name.clone(),
                    _ => continue,
                },
                _ => continue,
            };
            found.push(Static {
                object: member.name.clone(),
                symbol: symbol.name,
                section,
                size: symbol.size,
            });
        }
    }
    if objects == 0 {
        return Err("no objects in it".into());
    }
    Ok(found)
}

/// The crate an rlib's file name names: `paperpaw_sdk` for
/// `libpaperpaw_sdk-0123abcd.rlib`.
pub fn crate_name(path: &str) -> &str {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let file = file.strip_prefix("lib").unwrap_or(file);
    let file = file.strip_suffix(".rlib").unwrap_or(file);
    file.rsplit_once('-').map_or(file, |(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::build::Builder;
    use crate::elf::{EM_RISCV, ET_REL, SHF_ALLOC, SHF_WRITE};

    /// A member's header and data.
    fn member(archive: &mut Vec<u8>, name: &str, data: &[u8]) {
        let head = format!(
            "{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            0,
            0,
            0,
            644,
            data.len()
        );
        archive.extend_from_slice(head.as_bytes());
        archive.extend_from_slice(data);
        if data.len() % 2 == 1 {
            archive.push(b'\n');
        }
    }

    fn object(writable: bool) -> Vec<u8> {
        let mut b = Builder::new(EM_RISCV, ET_REL);
        let ro = b.section(".rodata.TABLE", SHF_ALLOC, 0, &[1; 8], 4);
        b.symbol("TABLE", 0, 8, ro as u16, false);
        if writable {
            let bss = b.nobits(".bss.COUNTER", SHF_ALLOC | SHF_WRITE, 0, 4);
            b.symbol(".bss.COUNTER", 0, 0, bss as u16, true);
            b.symbol("COUNTER", 0, 4, bss as u16, false);
        }
        b.build()
    }

    /// An rlib as rustc writes one: the symbol table, the long names, an
    /// object with a long name, the crate's metadata.
    fn rlib(writable: bool) -> Vec<u8> {
        let long = "paperpaw_sdk-0123abcd.paperpaw_sdk.4f2a-cgu.0.rcgu.o/\n";
        let mut archive = MAGIC.to_vec();
        member(&mut archive, "/", &[0, 0, 0, 0]);
        member(&mut archive, "//", long.as_bytes());
        member(&mut archive, "/0", &object(writable));
        member(&mut archive, "lib.rmeta/", b"rust\0\0\0\x09 not an object");
        archive
    }

    #[test]
    fn members_are_read_with_their_names() {
        let archive = rlib(false);
        let members = members(&archive).unwrap();
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "paperpaw_sdk-0123abcd.paperpaw_sdk.4f2a-cgu.0.rcgu.o",
                "lib.rmeta"
            ]
        );
        assert!(elf::is_elf(members[0].data));
        assert_eq!(super::members(b"\x7fELF").unwrap_err(), "not an ar archive");
        assert_eq!(
            super::members(&archive[..archive.len() - 3]).unwrap_err(),
            "the archive is cut short"
        );
    }

    #[test]
    fn a_static_in_writable_memory_is_found() {
        assert_eq!(statics(&rlib(false)).unwrap(), []);
        assert_eq!(
            statics(&rlib(true)).unwrap(),
            [Static {
                object: "paperpaw_sdk-0123abcd.paperpaw_sdk.4f2a-cgu.0.rcgu.o".into(),
                symbol: "COUNTER".into(),
                section: ".bss.COUNTER".into(),
                size: 4,
            }]
        );
    }

    #[test]
    fn an_archive_without_objects_is_refused() {
        // Built for LTO: bitcode where the object would be.
        let mut archive = MAGIC.to_vec();
        member(&mut archive, "lib.rmeta/", b"rust");
        member(&mut archive, "sdk.rcgu.o/", b"BC\xC0\xDE\x35\x14\x00\x00");
        let error = statics(&archive).unwrap_err();
        assert!(error.starts_with("sdk.rcgu.o: LLVM bitcode"), "{error}");

        let mut archive = MAGIC.to_vec();
        member(&mut archive, "lib.rmeta/", b"rust");
        assert_eq!(statics(&archive).unwrap_err(), "no objects in it");
    }

    #[test]
    fn the_crate_is_named_by_its_file() {
        assert_eq!(
            crate_name(
                "target/riscv32imc-unknown-none-elf/release/deps/libpaperpaw_sdk-0123abcd.rlib"
            ),
            "paperpaw_sdk"
        );
        assert_eq!(
            crate_name(r"deps\libpaperpaw_i18n-ff.rlib"),
            "paperpaw_i18n"
        );
        assert_eq!(crate_name("plain.a"), "plain.a");
    }
}
