//! Release assets that tools take from a release (the desktop emulators,
//! the handbook): signed apart with the firmware's key, as a flash image
//! is, but over [`DOMAIN`] and then the file, so an asset's signature
//! never holds as a firmware's or a flash image's, nor theirs as an
//! asset's. A tool checks one as `paperpaw_update::check_apart` checks a
//! flash image, with [`DOMAIN`] before the file's bytes.

use crate::secret_key;

/// What an asset's signature signs before the asset's bytes.
pub const DOMAIN: &[u8] = b"paperpaw-asset-v1\n";

/// The message an asset's signature signs: [`DOMAIN`], then `data`.
fn message(data: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(DOMAIN.len() + data.len());
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(data);
    message
}

pub fn sign(file: &str, signature: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = secret_key(key_file)?;
    let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let trailer =
        paperpaw_update::sign(&message(&data), &seed).map_err(|e| format!("{file}: {e}"))?;
    std::fs::write(signature, trailer.to_bytes()).map_err(|e| format!("{signature}: {e}"))?;
    println!(
        "{signature}: {} bytes signed as an asset by {}",
        data.len(),
        paperpaw_update::fingerprint(&trailer.key)
    );
    Ok(())
}

pub fn check(file: &str, signature: &str) -> Result<(), String> {
    let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let sig = std::fs::read(signature).map_err(|e| format!("{signature}: {e}"))?;
    let trailer = paperpaw_update::check_apart(&message(&data), &sig)
        .map_err(|e| format!("{file}: not an asset signed so ({e})"))?;
    println!("asset:       {} bytes", data.len());
    println!(
        "signer:      {} ({})",
        paperpaw_update::fingerprint(&trailer.key),
        paperpaw_update::key_hex(&trailer.key)
    );
    println!("signature:   holds");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An asset's signature holds for the asset alone, never as a flash
    /// image's; a flash image's never as an asset's.
    #[test]
    fn asset_signatures_are_apart_from_firmware_signatures() {
        let seed = [7u8; 32];
        let data = b"an emulator, zipped";
        let asset = paperpaw_update::sign(&message(data), &seed)
            .unwrap()
            .to_bytes();
        assert!(paperpaw_update::check_apart(&message(data), &asset).is_ok());
        assert!(
            paperpaw_update::check_apart(data, &asset).is_err(),
            "not as a flash image's"
        );
        let flash = paperpaw_update::sign(data, &seed).unwrap().to_bytes();
        assert!(
            paperpaw_update::check_apart(&message(data), &flash).is_err(),
            "nor the other way"
        );
        assert!(paperpaw_update::check_apart(&message(b"another"), &asset).is_err());
    }
}
