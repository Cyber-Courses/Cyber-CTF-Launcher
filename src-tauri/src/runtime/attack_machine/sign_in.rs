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

use crate::error::{Error, Result};
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

/// The salt's random bytes: 12, its 16 characters in the hash (SHA-crypt's longest).
const SALT_BYTES: usize = 12;

/// `password`'s SHA-512 crypt hash (`$6$rounds=5000$<salt>$<hash>`: what `/etc/shadow` holds, and
/// `chpasswd -e` takes), by the `sha-crypt` crate (`RustCrypto`) at its default rounds, its salt
/// from the operating system's random generator.
fn hash(password: &str) -> Result<String> {
    use rand::TryRng as _;
    use sha_crypt::PasswordHasher as _;
    let mut salt = [0u8; SALT_BYTES];
    rand::rngs::SysRng.try_fill_bytes(&mut salt).map_err(|e| Error::Invalid(format!("the system gave no random salt: {e}")))?;
    let hash = sha_crypt::ShaCrypt::SHA512
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|e| Error::Invalid(format!("the attack box's password couldn't be hashed: {e}")))?;
    Ok(hash.as_str().to_owned())
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
    let hashed = hash(&password)?;
    secrets::set_named(&entry(dir), &password)?;
    Ok(hashed)
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

    /// Whether `hash` (a crypt hash) is `password`'s, as the crate checks it.
    fn verifies(password: &str, hash: &str) -> bool {
        use sha_crypt::PasswordVerifier as _;
        let hash = sha_crypt::PasswordHashRef::new(hash).expect("a crypt hash");
        sha_crypt::ShaCrypt::SHA512.verify_password(password.as_bytes(), hash).is_ok()
    }

    #[test]
    fn the_hash_is_sha512_crypt_as_openssl_and_glibc_make_it() {
        use sha_crypt::{CustomizedPasswordHasher as _, Params};
        // `printf %s <password> | openssl passwd -6 -salt <salt> -stdin`: the first is the
        // reference's own (Drepper's SHA-crypt), the second a password longer than a digest. The
        // crate takes each, and refuses another password.
        let openssl = [
            ("Hello world!", "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1"),
            (
                "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789-long",
                "$6$Ab3/xY.z0123456$T3m9IgK/KG8RgqimjGQDIcX79mrpX7TGAhkMtThOUmoBscYmCG6jSjI5pb11Q9aqTFM2spulWmtMapy.OQeNp1",
            ),
            ("k7mPq-ZrT4w-Hn8Xc-Ls2Vd", "$6$0123456789abcdef$ofB0elLlK2vJI4M8vL3YpO5oegGXyVGEH6EntIjglU/mS1kHecuy5ZBL4ZN0S.aXfnQ5ZLTPwYD5M8riatiLD0"),
        ];
        for (password, hash) in openssl {
            assert!(verifies(password, hash), "{password}");
            assert!(!verifies("not-it", hash), "{password}");
        }
        // The crate's own hash, as `hash` makes it (its rounds written out), for the salt it makes
        // of 12 bytes: the digest openssl makes for that salt (`-salt XZbMZ7rMoNK9n34P`).
        let password = b"k7mpq-zrt4w-hn8xc-ls2vd";
        let made = sha_crypt::ShaCrypt::SHA512.hash_password_customized(password, b"cyberctf-sal", None, None, Params::RECOMMENDED).unwrap();
        let openssl = "$6$XZbMZ7rMoNK9n34P$eNhifv2Ihpd0/B0NXuFHXfOcjID582gRPMmYXcp9.y3Yos9JYoq.gJ/qghYhxvrYTq4ssC9QS96p8Aa0WqSCE.";
        assert_eq!(made.as_str(), openssl.replacen("$6$", "$6$rounds=5000$", 1));
        // A new one: the default rounds, 16 characters of new salt each time, the password's.
        let (a, b) = (hash("same-password").unwrap(), hash("same-password").unwrap());
        assert_ne!(a, b, "a new salt each time");
        let fields: Vec<&str> = a.split('$').collect();
        assert_eq!(fields[..3], ["", "6", "rounds=5000"], "{a}");
        assert_eq!(fields[3].len(), 16, "{a}");
        assert!(verifies("same-password", &a) && !verifies("same-passwore", &a));
    }

    #[test]
    fn a_new_password_is_kept_for_its_lab_registered_with_the_redaction_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("cyberctf-sign-in-{}", rand::random::<u32>()));
        assert_eq!(kept(&dir).unwrap(), None, "none before the box is added");
        let hashed = new(&dir).unwrap();
        let password = kept(&dir).unwrap().expect("kept");
        assert!(verifies(&password, &hashed), "the box gets the kept password's hash");
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
