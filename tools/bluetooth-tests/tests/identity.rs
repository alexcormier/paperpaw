//! The Pro shell's real bond codecs and RPA matching, executed on the host.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
extern crate alloc;
#[path = "../../../shells/xteink/src/bluetooth/identity.rs"]
mod identity;
use identity::{address_text, bond_of, connection_address, keys_text, reported_address};
use paperpaw_core::Bond;
use trouble_host::prelude::*;

fn paired() -> (Bond, BondInformation) {
    // Known-valid IRK/RPA fixture from trouble-host's crypto::rpa_test.
    let irk = IdentityResolvingKey::new(0x8b3958c158ed64467bd27bc90d3cf54d).unwrap();
    let old = BdAddr::new([0x92, 0xf2, 0x8f, 0x84, 0x72, 0x4f]);
    assert!(irk.resolve_address(&old));
    let info = BondInformation {
        ltk: LongTermKey::new(1),
        identity: Identity {
            addr: Address::new(AddrKind::PUBLIC, BdAddr::new([6, 5, 4, 3, 2, 1])),
            irk: Some(irk),
        },
        is_bonded: true,
        security_level: SecurityLevel::Encrypted,
    };
    let bond = Bond {
        address: address_text(AddrKind::RANDOM, old),
        keys: keys_text(&info),
    };
    (bond, info)
}
#[test]
fn resolved_and_rotated_addresses_keep_the_existing_kernel_device() {
    let (bond, info) = paired();
    let watched = vec![bond.address.clone()];
    assert_eq!(bond_of(&bond).unwrap().identity, info.identity);
    // A second AES-derived RPA for the same public test IRK.
    let next = BdAddr::new([0xb9, 0x03, 0x31, 0x13, 0x13, 0x52]);
    assert!(info.identity.irk.unwrap().resolve_address(&next));
    assert_ne!(address_text(AddrKind::RANDOM, next), bond.address);
    for (kind, address) in [
        (AddrKind::new(2), info.identity.addr.addr),
        (AddrKind::RANDOM, next),
    ] {
        assert_eq!(
            reported_address(kind, address, std::slice::from_ref(&bond), Some(&watched)),
            Some(bond.address.clone())
        );
        // Manual scans must not create another DeviceId for the same bond.
        assert_eq!(
            reported_address(kind, address, std::slice::from_ref(&bond), None),
            Some(bond.address.clone())
        );
    }
    // Found alone is not enough: the connection targets the bond identity.
    assert_eq!(
        connection_address(&bond.address, std::slice::from_ref(&bond)),
        Some(info.identity.addr)
    );
}
#[test]
fn public_and_static_random_devices_keep_their_address() {
    for (kind, report, bytes) in [(0, 2, [6, 5, 4, 3, 2, 1]), (1, 3, [6, 5, 4, 3, 2, 0xc1])] {
        let addr = Address::new(AddrKind::new(kind), BdAddr::new(bytes));
        let info = BondInformation {
            ltk: LongTermKey::new(1),
            identity: Identity::from(addr),
            is_bonded: true,
            security_level: SecurityLevel::Encrypted,
        };
        let bond = Bond {
            address: address_text(addr.kind, addr.addr),
            keys: keys_text(&info),
        };
        let watched = vec![bond.address.clone()];
        assert_eq!(
            reported_address(
                AddrKind::new(report),
                addr.addr,
                std::slice::from_ref(&bond),
                Some(&watched)
            ),
            Some(bond.address.clone())
        );
        assert_eq!(
            connection_address(&bond.address, std::slice::from_ref(&bond)),
            Some(addr)
        );
    }
}
#[test]
fn watch_rejects_other_peers_and_invalid_or_mislabelled_reports() {
    let (bond, info) = paired();
    let watched = vec![bond.address.clone()];
    let known_rpa = BdAddr::new([0x92, 0xf2, 0x8f, 0x84, 0x72, 0x4f]);
    let mut other_info = info.clone();
    other_info.identity.irk = IdentityResolvingKey::new(123);
    assert!(
        !other_info
            .identity
            .match_address(&Address::new(AddrKind::RANDOM, known_rpa))
    );
    let other_bond = Bond {
        address: bond.address.clone(),
        keys: keys_text(&other_info),
    };
    assert_eq!(
        reported_address(AddrKind::RANDOM, known_rpa, &[other_bond], Some(&watched)),
        None
    );
    // An IRK must not authenticate public or identity-kind RPA-shaped bytes.
    for kind in [0, 2, 3, 4, 255] {
        assert_eq!(
            reported_address(
                AddrKind::new(kind),
                known_rpa,
                std::slice::from_ref(&bond),
                Some(&watched)
            ),
            None
        );
    }
    // Invalid kinds must also fail direct identity-address comparisons.
    for kind in [4, 255] {
        assert_eq!(
            reported_address(
                AddrKind::new(kind),
                info.identity.addr.addr,
                std::slice::from_ref(&bond),
                None
            ),
            None
        );
    }
    assert_eq!(
        reported_address(
            AddrKind::new(2),
            info.identity.addr.addr,
            std::slice::from_ref(&bond),
            Some(&[])
        ),
        None
    );
    assert_eq!(
        reported_address(
            AddrKind::new(2),
            info.identity.addr.addr,
            std::slice::from_ref(&bond),
            Some(&["another peer".into()])
        ),
        None
    );
}
#[test]
fn ordinary_scan_and_unbonded_connection_still_use_the_observed_address() {
    let (bond, _) = paired();
    let addr = Address::new(AddrKind::PUBLIC, BdAddr::new([10, 20, 30, 40, 50, 60]));
    let text = address_text(addr.kind, addr.addr);
    assert_eq!(
        reported_address(addr.kind, addr.addr, std::slice::from_ref(&bond), None),
        Some(text.clone())
    );
    assert_eq!(
        connection_address(&text, std::slice::from_ref(&bond)),
        Some(addr)
    );
    let broken = Bond {
        address: bond.address.clone(),
        keys: "not a readable bond".into(),
    };
    assert_eq!(
        connection_address(&broken.address, std::slice::from_ref(&broken)),
        None
    );
}
