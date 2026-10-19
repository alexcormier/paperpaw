//! `paperpaw-sign`: signs firmware images as update files (see
//! `paperpaw-update`), makes signing keys and shows what an update file
//! holds.
//!
//! A device installs updates signed with PaperPaw's key, and those signed
//! with a key its reader chose to trust (their own builds).

use std::process::ExitCode;

use paperpaw_update::{AppInfo, Check, PLUGINS_AT, Plugins, TRAILER_LEN, Trailer};

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
        ["key", file] => key(file),
        ["sign", image, update] => sign(image, update, None),
        ["sign", image, update, "--key", file] => sign(image, update, Some(file)),
        ["show", update] => show(update),
        ["sign-apart", image, signature] => sign_apart(image, signature, None),
        ["sign-apart", image, signature, "--key", file] => sign_apart(image, signature, Some(file)),
        ["check", image, signature] => check(image, signature),
        ["plugins", image] => plugins(image),
        ["-h" | "--help"] => {
            print!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unexpected arguments\n\n{USAGE}")),
    }
}

fn key(file: &str) -> Result<(), String> {
    if std::path::Path::new(file).exists() {
        return Err(format!("{file} exists: a key is never overwritten"));
    }
    let pair = ed25519_compact::KeyPair::generate();
    let seed: [u8; 32] = *pair.sk.seed();
    std::fs::write(file, format!("{}\n", paperpaw_update::key_hex(&seed)))
        .map_err(|e| format!("{file}: {e}"))?;
    let public = paperpaw_update::public_key(&seed);
    println!("public key:  {}", paperpaw_update::key_hex(&public));
    println!("fingerprint: {}", paperpaw_update::fingerprint(&public));
    Ok(())
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

fn sign_apart(image: &str, signature: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = secret_key(key_file)?;
    let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    let trailer = paperpaw_update::sign(&data, &seed).map_err(|e| format!("{image}: {e}"))?;
    std::fs::write(signature, trailer.to_bytes()).map_err(|e| format!("{signature}: {e}"))?;
    println!(
        "{signature}: {} bytes signed by {}",
        data.len(),
        paperpaw_update::fingerprint(&trailer.key)
    );
    Ok(())
}

fn check(image: &str, signature: &str) -> Result<(), String> {
    let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    let sig = std::fs::read(signature).map_err(|e| format!("{signature}: {e}"))?;
    let trailer = paperpaw_update::check_apart(&data, &sig).map_err(|e| format!("{image}: {e}"))?;
    println!("image:       {} bytes", data.len());
    println!(
        "signer:      {} ({})",
        paperpaw_update::fingerprint(&trailer.key),
        paperpaw_update::key_hex(&trailer.key)
    );
    println!("signature:   holds");
    Ok(())
}

fn sign(image: &str, update: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = secret_key(key_file)?;
    let mut data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    let info = AppInfo::read(&data).map_err(|e| format!("{image}: {e}"))?;
    let trailer = paperpaw_update::sign(&data, &seed).map_err(|e| format!("{image}: {e}"))?;
    data.extend_from_slice(&trailer.to_bytes());
    std::fs::write(update, &data).map_err(|e| format!("{update}: {e}"))?;
    println!(
        "{update}: {} {}, signed by {}",
        info.project,
        info.version,
        paperpaw_update::fingerprint(&trailer.key)
    );
    Ok(())
}

fn show(update: &str) -> Result<(), String> {
    let data = std::fs::read(update).map_err(|e| format!("{update}: {e}"))?;
    let split = data
        .len()
        .checked_sub(TRAILER_LEN)
        .ok_or_else(|| format!("{update}: too short"))?;
    let trailer =
        Trailer::read(&data[split..], data.len() as u64).map_err(|e| format!("{update}: {e}"))?;
    let info = AppInfo::read(&data[..split]).map_err(|e| format!("{update}: {e}"))?;
    let mut check = Check::new(&trailer).map_err(|e| format!("{update}: {e}"))?;
    check.absorb(&data[..split]);
    let holds = check.finish();
    println!("firmware:    {} {}", info.project, info.version);
    println!("chip:        {}", info.chip);
    println!("image:       {} bytes", trailer.image_len);
    match plugins_of(&data[..split]) {
        Some(list) => println!("plugins:     {} built in", list.iter().count()),
        None => println!("plugins:     not listed"),
    }
    println!(
        "signer:      {} ({})",
        paperpaw_update::fingerprint(&trailer.key),
        paperpaw_update::key_hex(&trailer.key)
    );
    match holds {
        Ok(()) => println!("signature:   holds"),
        Err(e) => return Err(format!("{update}: {e}")),
    }
    Ok(())
}

/// The plugin list of an app image (or an update file, which begins with
/// one).
fn plugins_of(image: &[u8]) -> Option<Plugins> {
    Plugins::read(image.get(PLUGINS_AT..)?)
}

fn plugins(image: &str) -> Result<(), String> {
    let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    AppInfo::read(&data).map_err(|e| format!("{image}: {e}"))?;
    let list = plugins_of(&data).ok_or_else(|| format!("{image}: no list of plugins"))?;
    if list.is_empty() {
        return Err(format!("{image}: no plugins listed"));
    }
    print!("{}", list.text());
    Ok(())
}
