//! `paperpaw-sign`: signs firmware images as update files (see
//! `paperpaw-update`), makes signing keys and shows what an update file
//! holds.
//!
//! A device installs updates signed with PaperPaw's key, and those signed
//! with a key its reader chose to trust (their own builds).

use std::process::ExitCode;

mod firmware;

const USAGE: &str = "\
paperpaw-sign: sign PaperPaw firmware updates

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
