//! The code: kept as `kind:salt:hash` among the passwords and keys, the
//! hash a salted SHA-1 taken many times over, so the code itself is
//! nowhere on the device.

use alloc::format;
use alloc::string::{String, ToString};

use paperpaw_digest::sha1::Sha1;
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::{Command, Env};

use crate::{KIND, KIND_PREFIX};

/// The secret's id.
const SECRET: &str = "lock.code";
/// How often the hash is taken.
const ROUNDS: u32 = 2000;

/// The hash of `code` with `salt`.
pub(crate) fn hash(salt: &str, code: &str) -> String {
    let mut digest = {
        let mut sha = Sha1::new();
        sha.update(salt.as_bytes());
        sha.update(code.as_bytes());
        sha.finish()
    };
    for _ in 1..ROUNDS {
        let mut sha = Sha1::new();
        sha.update(salt.as_bytes());
        sha.update(&digest);
        digest = sha.finish();
    }
    paperpaw_digest::sha1::hex(&digest)
}

/// The kind of the lock in use (`pin`, `pattern`, `password`): one whose
/// code is kept and that the setting names.
pub(crate) fn active(env: &Env<'_>) -> Option<String> {
    let setting = env.settings.read(&KIND);
    let kind = setting.strip_prefix(KIND_PREFIX).unwrap_or(setting);
    kept_kind(env).filter(|kept| kind != "off" && kept == kind)
}

/// The kind of the code kept, if one is whole (kind, salt and hash): a
/// damaged one locks nothing, as no code could open it.
pub(crate) fn kept_kind(env: &Env<'_>) -> Option<String> {
    let stored = env.secrets.get(SECRET)?;
    let mut parts = stored.splitn(3, ':');
    let (Some(kind), Some(salt), Some(hashed)) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if salt.is_empty() || hashed.is_empty() {
        return None;
    }
    crate::KINDS
        .contains(&kind)
        .then(|| kind.to_string())
        .filter(|kind| kind != "off")
}

/// Whether `code` is the code in use.
pub(crate) fn check(env: &Env<'_>, code: &str) -> bool {
    let Some(stored) = env.secrets.get(SECRET) else {
        return false;
    };
    let mut parts = stored.splitn(3, ':');
    let (Some(_), Some(salt), Some(hashed)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    hash(salt, code) == hashed
}

/// The command that keeps `code` for a lock of `kind`.
pub(crate) fn keep(env: &Env<'_>, kind: &str, code: &str) -> Command {
    // Different on every device and every time.
    let salt = {
        let mut sha = Sha1::new();
        sha.update(env.device.id.as_bytes());
        sha.update(&env.now.millis().to_le_bytes());
        sha.update(code.as_bytes());
        let mut text = sha.hex();
        text.truncate(16);
        text
    };
    let value = format!("{kind}:{salt}:{}", hash(&salt, code));
    Command::SetSecret(SecretChange {
        id: SECRET.to_string(),
        label: String::from("lock.secret"),
        value: Some(value),
        // A hash: nothing to type anew in the list.
        changeable: false,
    })
}

/// The command that forgets the code.
pub(crate) fn forget() -> Command {
    Command::SetSecret(SecretChange::remove(SECRET))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_depend_on_salt_and_code() {
        let a = hash("salt", "1234");
        assert_eq!(a.len(), 40);
        assert_eq!(a, hash("salt", "1234"));
        assert_ne!(a, hash("pepper", "1234"));
        assert_ne!(a, hash("salt", "1235"));
    }
}
