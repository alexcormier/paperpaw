//! The firmware's commands: signing keys, update files, whole flash
//! images signed apart, and the plugin crates an image has built in.

use paperpaw_update::{AppInfo, Check, PLUGINS_AT, Plugins, TRAILER_LEN, Trailer};

use crate::secret_key;

pub fn key(file: &str) -> Result<(), String> {
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

pub fn sign_apart(image: &str, signature: &str, key_file: Option<&str>) -> Result<(), String> {
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

pub fn check(image: &str, signature: &str) -> Result<(), String> {
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

pub fn sign(image: &str, update: &str, key_file: Option<&str>) -> Result<(), String> {
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

pub fn show(update: &str) -> Result<(), String> {
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

pub fn plugins(image: &str) -> Result<(), String> {
    let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    AppInfo::read(&data).map_err(|e| format!("{image}: {e}"))?;
    let list = plugins_of(&data).ok_or_else(|| format!("{image}: no list of plugins"))?;
    if list.is_empty() {
        return Err(format!("{image}: no plugins listed"));
    }
    print!("{}", list.text());
    Ok(())
}
