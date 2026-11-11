//! The password of the box's player account ([`super::PLAYER`]), for wherever the box asks for
//! one: the screen locker or `LightDM`'s login on a desktop box's screen, a VM's console login, `su`,
//! SSH. The launcher makes one per box when it adds the box ([`new`]): random and easy to type
//! (lowercase letters and digits, no look-alikes, in groups). It keeps it with its other secrets
//! (`runtime::server::secrets`: the OS keychain, a 0600 file in a dev build), under the lab's
//! name, and registers it with the log redaction ([`crate::platform::redact`]) wherever it holds
//! it. The box gets only its SHA-512 crypt hash, in a root-only file its set-up hands to
//! `chpasswd -e` on its standard input ([`HASH_FILE`]): the password itself is never in the box,
//! the lab's files, the run's or a command line. The lab page and the box's screen window show it
//! ([`SignIn`]); `sudo` still asks for none.

use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha512};

use crate::error::Result;
use crate::platform::redact;
use crate::runtime::server::secrets;

/// Where the box holds the password's hash, root's only (`0400`): the box's spec places it there
/// (`files`), before its set-up reads it.
pub(crate) const HASH_FILE: &str = "/etc/cyberctf/player-password";

/// What the password is made of: lowercase letters and digits without the look-alikes (`l`, `1`,
/// `o`, `0`), so it reads and types the same on any keyboard layout, with no Shift.
const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// Its groups, and the characters in each: 4 groups of 5 (100 bits), joined by `-`.
const GROUPS: usize = 4;
const GROUP: usize = 5;

/// How the player signs in to the box where it asks: the account and its password.
#[derive(Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SignIn {
    /// The account, `player` (`attack_machine::PLAYER`).
    pub user: String,
    /// Its password.
    pub password: String,
}

/// The password never shows in a debug print.
impl std::fmt::Debug for SignIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignIn").field("user", &self.user).field("password", &"<redacted>").finish()
    }
}

/// A new password: [`GROUPS`] groups of [`GROUP`] characters of [`ALPHABET`], from the OS-seeded
/// generator, joined by `-` (`k7mpq-zrt4w-hn8xc-ls2vd`).
fn generate() -> String {
    let group = || (0..GROUP).map(|_| char::from(ALPHABET[rand::random_range(0..ALPHABET.len())])).collect::<String>();
    (0..GROUPS).map(|_| group()).collect::<Vec<_>>().join("-")
}

/// The characters of a crypt hash's salt and its encoding.
const CRYPT64: &[u8] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `password`'s SHA-512 crypt hash with `salt` (at most 16 characters of [`CRYPT64`]) at the
/// default 5000 rounds (`$6$<salt>$<hash>`), as glibc's `crypt` and `openssl passwd -6` make it:
/// what `/etc/shadow` holds, and `chpasswd -e` takes.
fn sha512_crypt(password: &str, salt: &str) -> String {
    const ROUNDS: usize = 5000;
    let (key, salt) = (password.as_bytes(), &salt.as_bytes()[..salt.len().min(16)]);
    // `len` bytes of `digest`, repeated: whole digests, then the start of one.
    let spread = |digest: &[u8], len: usize| -> Vec<u8> { digest.iter().copied().cycle().take(len).collect() };
    let alternate = Sha512::new().chain_update(key).chain_update(salt).chain_update(key).finalize();
    let mut first = Sha512::new().chain_update(key).chain_update(salt);
    first.update(spread(&alternate, key.len()));
    let mut bits = key.len();
    while bits > 0 {
        if bits & 1 == 1 {
            first.update(alternate);
        } else {
            first.update(key);
        }
        bits >>= 1;
    }
    let mut digest = first.finalize();
    let key_digest = key.iter().fold(Sha512::new(), |d, _| d.chain_update(key)).finalize();
    let key_bytes = spread(&key_digest, key.len());
    let salt_digest = (0..16 + usize::from(digest[0])).fold(Sha512::new(), |d, _| d.chain_update(salt)).finalize();
    let salt_bytes = spread(&salt_digest, salt.len());
    for round in 0..ROUNDS {
        let mut next = Sha512::new();
        if round % 2 == 1 {
            next.update(&key_bytes);
        } else {
            next.update(digest);
        }
        if round % 3 != 0 {
            next.update(&salt_bytes);
        }
        if round % 7 != 0 {
            next.update(&key_bytes);
        }
        if round % 2 == 1 {
            next.update(digest);
        } else {
            next.update(&key_bytes);
        }
        digest = next.finalize();
    }
    // Its 64 bytes in 21 groups of three, in crypt's order, then the last alone.
    let mut out = format!("$6${}$", String::from_utf8_lossy(salt));
    let mut put = |word: u32, chars: usize| {
        (0..chars).for_each(|i| out.push(char::from(CRYPT64[((word >> (6 * i)) & 0x3f) as usize])));
    };
    for group in 0..21 {
        let at = group * 22 % 63;
        put(u32::from(digest[at]) << 16 | u32::from(digest[(at + 21) % 63]) << 8 | u32::from(digest[(at + 42) % 63]), 4);
    }
    put(u32::from(digest[63]), 2);
    out
}

/// `password`'s SHA-512 crypt hash, with a new random salt.
fn hash(password: &str) -> String {
    let salt: String = (0..16).map(|_| char::from(CRYPT64[rand::random_range(0..CRYPT64.len())])).collect();
    sha512_crypt(password, &salt)
}

/// Its store's name for the box of the lab in `dir` (the lab's folder, named after its id).
fn entry(dir: &Path) -> String {
    format!("attack-box:{}", dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())
}

/// A new password for the box of the lab in `dir`, kept (registered with the redaction first):
/// the hash its set-up places in [`HASH_FILE`].
pub(super) fn new(dir: &Path) -> Result<String> {
    let password = generate();
    redact::register(&password);
    secrets::set_named(&entry(dir), &password)?;
    Ok(hash(&password))
}

/// The password kept for the box of the lab in `dir`, registered with the redaction; none when
/// the launcher keeps none.
pub(super) fn kept(dir: &Path) -> Result<Option<String>> {
    let password = secrets::get_named(&entry(dir))?;
    if let Some(p) = &password {
        redact::register(p);
    }
    Ok(password)
}

/// Forgets the box's password, once it is out of the lab in `dir`. A store that refuses (a locked
/// keychain) leaves it there, said in the log: a box is still taken out.
pub(super) fn forget(dir: &Path) {
    if let Err(e) = secrets::delete_named(&entry(dir)) {
        log::warn!("couldn't forget the attack box's password: {e}");
    }
}

/// The box's spec's file holding `hash` at [`HASH_FILE`], root's only, as `files` lists one.
pub(super) fn hash_file(hash: &str) -> serde_yaml_ng::Value {
    let mut file = serde_yaml_ng::Mapping::new();
    file.insert("path".into(), HASH_FILE.into());
    file.insert("content".into(), format!("{hash}\n").into());
    file.insert("mode".into(), "0400".into());
    serde_yaml_ng::Value::Mapping(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_is_23_characters_of_five_letter_groups_without_look_alikes() {
        let passwords: Vec<String> = (0..200).map(|_| generate()).collect();
        for p in &passwords {
            assert_eq!(p.len(), 23, "{p}");
            let groups: Vec<&str> = p.split('-').collect();
            assert_eq!(groups.len(), 4, "{p}");
            assert!(groups.iter().all(|g| g.len() == 5 && g.bytes().all(|b| ALPHABET.contains(&b))), "{p}");
            assert!(!p.contains(['l', '1', 'o', '0', 'I', 'O']), "no look-alikes: {p}");
        }
        let distinct: std::collections::BTreeSet<_> = passwords.iter().collect();
        assert_eq!(distinct.len(), passwords.len(), "each one new");
        assert_eq!(ALPHABET.len(), 32, "5 bits a character: 100 bits a password");
        let every: std::collections::BTreeSet<u8> = passwords.iter().flat_map(|p| p.bytes()).filter(|b| *b != b'-').collect();
        assert_eq!(every.len(), ALPHABET.len(), "every character of the alphabet comes up");
    }

    #[test]
    fn the_hash_is_sha512_crypt_as_openssl_and_glibc_make_it() {
        // `printf %s <password> | openssl passwd -6 -salt <salt> -stdin`; the first is the
        // reference's own (Drepper's SHA-crypt), the second a password longer than a digest.
        let cases = [
            ("Hello world!", "saltstring", "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1"),
            (
                "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789-long",
                "Ab3/xY.z0123456",
                "$6$Ab3/xY.z0123456$T3m9IgK/KG8RgqimjGQDIcX79mrpX7TGAhkMtThOUmoBscYmCG6jSjI5pb11Q9aqTFM2spulWmtMapy.OQeNp1",
            ),
            (
                "k7mPq-ZrT4w-Hn8Xc-Ls2Vd",
                "0123456789abcdef",
                "$6$0123456789abcdef$ofB0elLlK2vJI4M8vL3YpO5oegGXyVGEH6EntIjglU/mS1kHecuy5ZBL4ZN0S.aXfnQ5ZLTPwYD5M8riatiLD0",
            ),
        ];
        for (password, salt, expected) in cases {
            assert_eq!(sha512_crypt(password, salt), expected, "{password}");
        }
        let (a, b) = (hash("same"), hash("same"));
        assert_ne!(a, b, "a new salt each time");
        let salt = a.split('$').nth(2).unwrap();
        assert!(salt.len() == 16 && salt.bytes().all(|c| CRYPT64.contains(&c)), "{a}");
        assert_eq!(sha512_crypt("same", salt), a);
    }

    #[test]
    fn a_new_password_is_kept_for_its_lab_registered_with_the_redaction_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("cyberctf-sign-in-{}", rand::random::<u32>()));
        assert_eq!(kept(&dir).unwrap(), None, "none before the box is added");
        let hashed = new(&dir).unwrap();
        let password = kept(&dir).unwrap().expect("kept");
        let salt = hashed.split('$').nth(2).unwrap();
        assert_eq!(sha512_crypt(&password, salt), hashed, "the box gets the kept password's hash");
        assert!(!hashed.contains(&password));
        let line = format!("the box's password is {password}");
        assert_eq!(crate::platform::redact::redact(&line), "the box's password is [redacted]");
        forget(&dir);
        assert_eq!(kept(&dir).unwrap(), None);
        let other = std::env::temp_dir().join("another-lab");
        assert_ne!(entry(&dir), entry(&other), "one per lab");
        assert!(entry(&dir).starts_with("attack-box:cyberctf-sign-in-"), "{}", entry(&dir));
    }

    #[test]
    fn a_sign_in_never_shows_its_password_in_a_debug_print() {
        let s = SignIn { user: "player".into(), password: "k7mpq-zrt4w-hn8xc-ls2vd".into() };
        assert_eq!(format!("{s:?}"), r#"SignIn { user: "player", password: "<redacted>" }"#);
    }
}
