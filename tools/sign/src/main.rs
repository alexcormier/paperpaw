//! `paperpaw-sign`: signs firmware images as update files (see
//! `paperpaw-update`), makes signing keys and shows what an update file
//! holds; makes native plugin files (`.ppn`, see `paperpaw-native`) from
//! linked plugins, signs them as their author and shows what they hold;
//! and checks that crates keep no mutable statics, as a plugin's copy of
//! the SDK must not.
//!
//! A device installs updates signed with PaperPaw's key, and those signed
//! with a key its reader chose to trust (their own builds). A plugin file
//! is signed by its author, whose key a device remembers at the first
//! install.

use std::process::ExitCode;

mod elf;
mod firmware;
mod pack;
mod plugin;
mod relocation;
mod statics;

const USAGE: &str = "\
paperpaw-sign: sign PaperPaw firmware updates, make and sign native plugins

USAGE:
    paperpaw-sign key <secret-key-file>
        Makes a new signing key, keeps it in the file (keep it safe) and
        prints its public key.
    paperpaw-sign sign <image.bin> <update.bin> [--key <secret-key-file>]
        Signs an app image (`espflash save-image`, not merged) as an update
        file. Without --key, the key comes from PAPERPAW_SIGNING_KEY (64
        hexadecimal digits).
    paperpaw-sign show <update.bin>
        Shows what the update holds, its signer, and whether its signature
        holds.
    paperpaw-sign sign-apart <flash.bin> <flash.bin.sig> [--key <secret-key-file>]
        Signs a whole flash image (as the web flasher writes it) with a
        signature file of its own; the key as for sign.
    paperpaw-sign check <flash.bin> <flash.bin.sig>
        Shows the signer of a whole flash image and whether its signature
        holds.
    paperpaw-sign plugins <image.bin>
        Lists the plugin crates an app image or update file has built in;
        fails if it lists none.
    paperpaw-sign pack <plugin.elf> <plugin.ppn>
        Makes a native plugin file from a plugin linked with
        paperpaw-plugin.x and --emit-relocs, or says what keeps it from
        moving (writable data, undefined symbols, addresses built into
        instructions, relative reaches out of the plugin).
    paperpaw-sign plugin <plugin.ppn>
        Shows what a plugin file holds, and its signer if
        <plugin.ppn>.sig lies beside it (failing if that does not hold).
    paperpaw-sign sign-plugin <plugin.ppn> [--key <secret-key-file>]
        Signs a plugin file as its author: writes <plugin.ppn>.sig and
        prints the key (64 hexadecimal digits) and the signature (128), as
        the plugin library lists them; the key as for sign. The signature
        is Ed25519 over \"PaperPaw plugin file\", a zero byte and the
        whole file; the .sig file is 104 bytes: PPNS, the format (1, two
        bytes little-endian), two zero bytes, the public key (32), the
        signature (64).
    paperpaw-sign check-plugin <plugin.ppn> [<plugin.ppn.sig>]
        Shows the signer of a plugin file and whether its signature holds.
    paperpaw-sign statics <crate.rlib>...
        Lists the symbols in writable memory (mutable statics) of the
        objects in rlibs (built without LTO); fails if there is one. A
        native plugin has no writable data, so the SDK's crates must keep
        none.
";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["key", file] => firmware::key(file),
        ["sign", image, update] => firmware::sign(image, update, None),
        ["sign", image, update, "--key", file] => firmware::sign(image, update, Some(file)),
        ["show", update] => firmware::show(update),
        ["sign-apart", image, signature] => firmware::sign_apart(image, signature, None),
        ["sign-apart", image, signature, "--key", file] => {
            firmware::sign_apart(image, signature, Some(file))
        }
        ["check", image, signature] => firmware::check(image, signature),
        ["plugins", image] => firmware::plugins(image),
        ["pack", elf, out] => plugin::pack(elf, out),
        ["plugin", file] => plugin::show(file),
        ["sign-plugin", file] => plugin::sign(file, None),
        ["sign-plugin", file, "--key", key] => plugin::sign(file, Some(key)),
        ["check-plugin", file] => plugin::check(file, None),
        ["check-plugin", file, signature] => plugin::check(file, Some(signature)),
        ["statics", archives @ ..] if !archives.is_empty() => plugin::statics(archives),
        ["-h" | "--help"] => {
            print!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unexpected arguments\n\n{USAGE}")),
    }
}

/// The secret key from `key_file`, else from PAPERPAW_SIGNING_KEY.
fn secret_key(key_file: Option<&str>) -> Result<[u8; 32], String> {
    let text = match key_file {
        Some(file) => std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?,
        None => std::env::var("PAPERPAW_SIGNING_KEY")
            .map_err(|_| "no --key and no PAPERPAW_SIGNING_KEY".to_string())?,
    };
    Ok(paperpaw_update::parse_key(&text).ok_or("the key is not 64 hexadecimal digits")?)
}
