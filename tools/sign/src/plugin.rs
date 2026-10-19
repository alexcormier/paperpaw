//! The commands for native plugins: making a plugin file, showing what
//! one holds, signing it as its author and checking its signature, and
//! the check that crates keep no mutable statics.

use paperpaw_digest::{hex, sha256::Sha256};
use paperpaw_native::Header;
use paperpaw_update::{PluginSignature, fingerprint, key_hex};

use crate::{secret_key, statics};

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}

/// A plugin file, checked whole.
fn plugin_file(path: &str) -> Result<(Vec<u8>, Header), String> {
    let file = read(path)?;
    let header = Header::check_file(&file).map_err(|e| format!("{path}: {e}"))?;
    Ok((file, header))
}

/// Where a plugin file's signature lies beside it.
fn signature_path(file: &str) -> String {
    format!("{file}.sig")
}

pub fn pack(elf: &str, out: &str) -> Result<(), String> {
    let packed = crate::pack::pack(&read(elf)?).map_err(|e| format!("{elf}: {e}"))?;
    write(out, &packed.file)?;
    let header = &packed.header;
    println!(
        "{out}: {} bytes of data, {} of code, {} words to relocate, fingerprint {:016x}, {}",
        header.code_at,
        header.image_len - header.code_at,
        header.relocs,
        header.fingerprint,
        header.chip
    );
    Ok(())
}

pub fn show(path: &str) -> Result<(), String> {
    let (file, header) = plugin_file(path)?;
    let mut sha = Sha256::new();
    sha.update(&file);
    println!("chip:        {} ({})", header.chip, header.chip.id());
    println!("fingerprint: {:016x}", header.fingerprint);
    println!(
        "image:       {} bytes: {} of data, {} of code, linked at {:#010x}, aligned to {}",
        header.image_len,
        header.code_at,
        header.image_len - header.code_at,
        header.base,
        header.align
    );
    println!(
        "entry:       at {}, the table's word at {}",
        header.entry, header.api
    );
    println!("relocations: {} words", header.relocs);
    println!(
        "file:        {} bytes, SHA-256 {}",
        file.len(),
        hex::encode(&sha.finish())
    );
    println!("digest:      {}", hex::encode(&header.sha256));
    let signature = signature_path(path);
    if !std::path::Path::new(&signature).exists() {
        println!("signature:   none beside it");
        return Ok(());
    }
    let signed = paperpaw_update::check_plugin(&file, &read(&signature)?)
        .map_err(|e| format!("{signature}: {e}"))?;
    print_signer(&signed);
    Ok(())
}

fn print_signer(signed: &PluginSignature) {
    println!(
        "signer:      {} ({})",
        fingerprint(&signed.key),
        key_hex(&signed.key)
    );
    println!("signature:   holds ({})", hex::encode(&signed.signature));
}

pub fn sign(path: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = secret_key(key_file)?;
    let (file, _) = plugin_file(path)?;
    let signed = paperpaw_update::sign_plugin(&file, &seed);
    let signature = signature_path(path);
    write(&signature, &signed.to_bytes())?;
    println!("{signature}: signed by {}", fingerprint(&signed.key));
    println!("key:         {}", key_hex(&signed.key));
    println!("signature:   {}", hex::encode(&signed.signature));
    Ok(())
}

pub fn check(path: &str, signature: Option<&str>) -> Result<(), String> {
    let signature = signature.map_or_else(|| signature_path(path), str::to_string);
    let (file, header) = plugin_file(path)?;
    let signed = paperpaw_update::check_plugin(&file, &read(&signature)?)
        .map_err(|e| format!("{signature}: {e}"))?;
    println!(
        "plugin:      {} bytes for the {}, fingerprint {:016x}",
        file.len(),
        header.chip,
        header.fingerprint
    );
    print_signer(&signed);
    Ok(())
}

pub fn statics(archives: &[&str]) -> Result<(), String> {
    let mut count = 0;
    for path in archives {
        let found = statics::statics(&read(path)?).map_err(|e| format!("{path}: {e}"))?;
        let name = statics::crate_name(path);
        for s in &found {
            println!(
                "{name}: {}: {} ({}, {} bytes)",
                s.object, s.symbol, s.section, s.size
            );
        }
        count += found.len();
    }
    if count > 0 {
        return Err(format!(
            "{count} mutable statics: a native plugin with these crates could not load"
        ));
    }
    println!("no mutable statics in {} archives", archives.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::{EM_RISCV, EM_XTENSA};
    use crate::pack::tests::packed;

    #[test]
    fn a_signed_plugin_checks_and_another_file_does_not() {
        let dir = std::env::temp_dir().join(format!("paperpaw-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let (key, file, other) = (path("key"), path("plugin.ppn"), path("other.ppn"));
        std::fs::write(
            &key,
            format!(
                "{}
",
                "07".repeat(32)
            ),
        )
        .unwrap();
        std::fs::write(&file, packed(EM_RISCV)).unwrap();
        std::fs::write(&other, packed(EM_XTENSA)).unwrap();

        sign(&file, Some(&key)).unwrap();
        let signature = signature_path(&file);
        assert_eq!(std::fs::read(&signature).unwrap().len(), 104);
        check(&file, None).unwrap();
        show(&file).unwrap();
        let error = check(&other, Some(&signature)).unwrap_err();
        assert!(error.ends_with("the signature does not hold"), "{error}");

        // A changed byte: the file no longer holds, before its signature.
        let mut changed = std::fs::read(&file).unwrap();
        changed[100] ^= 1;
        std::fs::write(&file, changed).unwrap();
        let error = check(&file, None).unwrap_err();
        assert!(error.ends_with("changed or damaged"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
