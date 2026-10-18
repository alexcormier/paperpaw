//! `quireink-sign`: signs firmware images as update files (see
//! `quireink-update`), makes signing keys and shows what an update file
//! holds.
//!
//! A device installs updates signed with QuireInk's key, and those signed
//! with a key its reader chose to trust (their own builds).

use std::process::ExitCode;

use quireink_update::{AppInfo, Check, TRAILER_LEN, Trailer};

const USAGE: &str = "\
quireink-sign: sign QuireInk firmware updates

USAGE:
    quireink-sign key <secret-key-file>
        Makes a new signing key, keeps it in the file (keep it safe) and
        prints its public key.
    quireink-sign sign <image.bin> <update.bin> [--key <secret-key-file>]
        Signs an app image (`espflash save-image`, not merged) as an update
        file. Without --key, the key comes from QUIREINK_SIGNING_KEY (64
        hexadecimal digits).
    quireink-sign show <update.bin>
        Shows what the update holds, its signer, and whether its signature
        holds.
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
    std::fs::write(file, format!("{}\n", quireink_update::key_hex(&seed)))
        .map_err(|e| format!("{file}: {e}"))?;
    let public = quireink_update::public_key(&seed);
    println!("public key:  {}", quireink_update::key_hex(&public));
    println!("fingerprint: {}", quireink_update::fingerprint(&public));
    Ok(())
}

fn sign(image: &str, update: &str, key_file: Option<&str>) -> Result<(), String> {
    let text = match key_file {
        Some(file) => std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?,
        None => std::env::var("QUIREINK_SIGNING_KEY")
            .map_err(|_| "no --key and no QUIREINK_SIGNING_KEY".to_string())?,
    };
    let seed = quireink_update::parse_key(&text).ok_or("the key is not 64 hexadecimal digits")?;
    let mut data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    let info = AppInfo::read(&data).map_err(|e| format!("{image}: {e}"))?;
    let trailer = quireink_update::sign(&data, &seed).map_err(|e| format!("{image}: {e}"))?;
    data.extend_from_slice(&trailer.to_bytes());
    std::fs::write(update, &data).map_err(|e| format!("{update}: {e}"))?;
    println!(
        "{update}: {} {}, signed by {}",
        info.project,
        info.version,
        quireink_update::fingerprint(&trailer.key)
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
    println!(
        "signer:      {} ({})",
        quireink_update::fingerprint(&trailer.key),
        quireink_update::key_hex(&trailer.key)
    );
    match holds {
        Ok(()) => println!("signature:   holds"),
        Err(e) => return Err(format!("{update}: {e}")),
    }
    Ok(())
}
