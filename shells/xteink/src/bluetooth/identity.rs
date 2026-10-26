//! Bluetooth addresses reported by the controller and identities kept in bonds.
//! Keep the kernel's existing address (and DeviceId) when a peer changes its RPA.

use alloc::format;
use alloc::string::String;
use paperpaw_core::Bond;
use trouble_host::prelude::*;

/// The address the kernel knows, restricted to the watched bonds where given.
/// The controller can report either an unresolved RPA or a resolved identity.
pub(super) fn reported_address(
    kind: AddrKind,
    address: BdAddr,
    bonds: &[Bond],
    watched: Option<&[String]>,
) -> Option<String> {
    // Only public, random and their controller-resolved identity forms.
    // Address equality in the host masks the kind's low bit, so check first:
    // an anonymous/invalid report must never become a known device.
    if kind.as_raw() > 3 {
        return None;
    }
    let observed = Address::new(kind, address);
    let known = bonds.iter().find_map(|bond| {
        if watched.is_some_and(|only| !only.contains(&bond.address)) {
            return None;
        }
        let identity = bond_of(bond)?.identity;
        // IRK resolution is for an actual random address, never public bytes
        // that happen to look like an RPA or a controller-resolved identity.
        let matches = identity.addr == observed
            || (kind == AddrKind::RANDOM && identity.match_address(&observed));
        matches.then(|| bond.address.clone())
    });
    let address = match known {
        Some(address) => address,
        None if watched.is_some() => return None,
        None => address_text(kind, address),
    };
    watched
        .is_none_or(|only| only.contains(&address))
        .then_some(address)
}

/// A saved logical address connects through its verified bond identity. The
/// kernel keeps its original address, even if pairing saw an old private one.
pub(super) fn connection_address(requested: &str, bonds: &[Bond]) -> Option<Address> {
    if let Some(bond) = bonds.iter().find(|bond| bond.address == requested) {
        return bond_of(bond).map(|bond| bond.identity.addr);
    }
    address_of(requested)
}

/// An address as the kernel keeps it: most significant byte first, with
/// `/r` for a random one.
pub(super) fn address_text(kind: AddrKind, address: BdAddr) -> String {
    let b = address.raw();
    let random = if kind.as_raw() & 1 == 1 { "/r" } else { "" };
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}{random}",
        b[5], b[4], b[3], b[2], b[1], b[0]
    )
}

/// The address `text` names ([`address_text`]).
pub(super) fn address_of(text: &str) -> Option<Address> {
    let (bytes, kind) = match text.strip_suffix("/r") {
        Some(bytes) => (bytes, AddrKind::RANDOM),
        None => (text, AddrKind::PUBLIC),
    };
    let mut address = [0; 6];
    let mut parts = bytes.split(':');
    for at in (0..6).rev() {
        address[at] = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(Address::new(kind, BdAddr::new(address)))
}

/// A bond's keys as the kernel keeps them: the long term key, the
/// identity resolving key (`-` for none), the security (0 to 2) and the
/// identity address, separated by dots.
pub(super) fn keys_text(bond: &BondInformation) -> String {
    let irk = bond
        .identity
        .irk
        .map_or_else(|| String::from("-"), |irk| format!("{:032x}", irk.0.get()));
    let security = match bond.security_level {
        SecurityLevel::NoEncryption => 0,
        SecurityLevel::Encrypted => 1,
        SecurityLevel::EncryptedAuthenticated => 2,
    };
    format!(
        "{:032x}.{irk}.{security}.{}",
        bond.ltk.0,
        address_text(bond.identity.addr.kind, bond.identity.addr.addr)
    )
}

/// The bond the kernel keeps, for the host ([`keys_text`]).
pub(super) fn bond_of(bond: &Bond) -> Option<BondInformation> {
    let mut parts = bond.keys.split('.');
    let ltk = u128::from_str_radix(parts.next()?, 16).ok()?;
    let irk = match parts.next()? {
        "-" => None,
        irk => IdentityResolvingKey::new(u128::from_str_radix(irk, 16).ok()?),
    };
    let security_level = match parts.next()? {
        "2" => SecurityLevel::EncryptedAuthenticated,
        "1" => SecurityLevel::Encrypted,
        _ => SecurityLevel::NoEncryption,
    };
    let address = parts
        .next()
        .and_then(address_of)
        .or_else(|| address_of(&bond.address))?;
    Some(BondInformation {
        ltk: LongTermKey::new(ltk),
        identity: Identity { addr: address, irk },
        is_bonded: true,
        security_level,
    })
}
