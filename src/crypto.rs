//! Pure-Rust application-layer envelope encryption for SubVault Format 6.
//!
//! - Master DEK: 256-bit random key wrapped independently by:
//!   1) Passphrase slot (`Argon2id` -> `XChaCha20-Poly1305`), and
//!   2) Offline recovery key slot (256-bit random `SVRK-...` with checksum -> `HKDF-SHA256` -> `XChaCha20-Poly1305`).
//! - Subkeys derived from DEK via `HKDF-SHA256(DEK, salt=vault_id, info=...)`:
//!   - `object_key`: streaming 64 KiB chunked `XChaCha20-Poly1305` (`SVENC001`) bound to `vault_id || object_id || chunk_idx || flags`
//!   - `field_key`: per-cell `XChaCha20-Poly1305` (`enc:v1:<base64>`) bound to `vault_id || table || row_id || column`
//!   - `journal_key`: per-journal `XChaCha20-Poly1305` (`SVJRN001`) bound to `vault_id || command_id`
//!   - `dedup_key`: keyed `HMAC-SHA256` for O(1) dedup/idempotency without leaking plaintext SHA-256 on disk.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::ids::hex_encode;
use crate::platform::fill_random;

pub const PROTECTION_PASSPHRASE: &str = "passphrase_aead_v1";
pub const PROTECTION_OS_DISK: &str = "os_disk_assumed";
pub const PROTECTION_SYNTHETIC: &str = "unanswered_synthetic_plaintext";

pub const OBJECT_MAGIC: &[u8; 8] = b"SVENC001";
pub const JOURNAL_MAGIC: &[u8; 8] = b"SVJRN001";
pub const FIELD_PREFIX: &str = "enc:v1:";

pub const CHUNK_PLAIN_SIZE: usize = 64 * 1024;
const CHUNK_TAG_SIZE: usize = 16;
const FLAG_MORE: u8 = 0x00;
const FLAG_FINAL: u8 = 0x01;

pub const DEFAULT_ARGON2_M_COST: u32 = 65_536;
pub const DEFAULT_ARGON2_T_COST: u32 = 3;
pub const DEFAULT_ARGON2_P_COST: u32 = 1;

const MIN_ARGON2_M_COST: u32 = 16_384; // 16 MiB
const MAX_ARGON2_M_COST: u32 = 524_288; // 512 MiB
const MIN_ARGON2_T_COST: u32 = 1;
const MAX_ARGON2_T_COST: u32 = 10;
const MIN_ARGON2_P_COST: u32 = 1;
const MAX_ARGON2_P_COST: u32 = 8;
const MIN_ARGON2_SALT_LEN: usize = 16;
const MAX_ARGON2_SALT_LEN: usize = 64;

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug)]
pub enum CryptoError {
    Io(io::Error),
    InvalidFormat(&'static str),
    AuthFailed,
    Tampered(&'static str),
    Rng(io::Error),
    Kdf(String),
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::Io(err) => write!(f, "io error: {err}"),
            CryptoError::InvalidFormat(msg) => write!(f, "invalid crypto format: {msg}"),
            CryptoError::AuthFailed => write!(f, "authentication failed"),
            CryptoError::Tampered(msg) => write!(f, "ciphertext authentication failed: {msg}"),
            CryptoError::Rng(err) => write!(f, "rng failed: {err}"),
            CryptoError::Kdf(msg) => write!(f, "kdf failed: {msg}"),
        }
    }
}

impl From<io::Error> for CryptoError {
    fn from(err: io::Error) -> Self {
        CryptoError::Io(err)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionMode {
    Passphrase,
    OsDisk,
}

impl ProtectionMode {
    pub fn as_meta(self) -> &'static str {
        match self {
            ProtectionMode::Passphrase => PROTECTION_PASSPHRASE,
            ProtectionMode::OsDisk => PROTECTION_OS_DISK,
        }
    }

    pub fn from_flag(raw: &str) -> Option<Self> {
        match raw {
            "passphrase" => Some(ProtectionMode::Passphrase),
            "os-disk" => Some(ProtectionMode::OsDisk),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
pub struct AuthInput {
    pub passphrase_file: Option<PathBuf>,
    pub passphrase_stdin: bool,
    pub recovery_key_file: Option<PathBuf>,
    pub recovery_key_stdin: bool,
    pub(crate) session_cipher: Option<VaultCipher>,
    pub(crate) direct_passphrase: Option<Zeroizing<String>>,
    pub(crate) direct_recovery_key: Option<Zeroizing<String>>,
}

impl AuthInput {
    pub(crate) fn from_cipher(cipher: VaultCipher) -> Self {
        Self {
            session_cipher: Some(cipher),
            ..Self::default()
        }
    }

    pub(crate) fn from_direct_passphrase(pass: String) -> Self {
        Self {
            direct_passphrase: Some(Zeroizing::new(pass)),
            ..Self::default()
        }
    }

    pub(crate) fn from_direct_recovery_key(key: String) -> Self {
        Self {
            direct_recovery_key: Some(Zeroizing::new(key)),
            ..Self::default()
        }
    }
}

#[derive(Clone, Default)]
pub struct NewPassphraseInput {
    pub file: Option<PathBuf>,
    pub stdin: bool,
    pub(crate) direct: Option<Zeroizing<String>>,
}

impl NewPassphraseInput {
    pub(crate) fn from_direct(pass: String) -> Self {
        Self {
            file: None,
            stdin: false,
            direct: Some(Zeroizing::new(pass)),
        }
    }
}

pub enum Credential {
    None,
    Passphrase(Zeroizing<String>),
    RecoveryKey(Zeroizing<String>),
}

#[derive(Debug)]
pub enum CredentialError {
    Conflict,
    Empty(&'static str),
    Io(io::Error),
}

impl AuthInput {
    pub fn resolve(&self) -> Result<Credential, CredentialError> {
        if self.direct_passphrase.is_some() && self.direct_recovery_key.is_some() {
            return Err(CredentialError::Conflict);
        }
        if let Some(ref p) = self.direct_passphrase {
            if p.is_empty() {
                return Err(CredentialError::Empty("empty_passphrase"));
            }
            return Ok(Credential::Passphrase(p.clone()));
        }
        if let Some(ref r) = self.direct_recovery_key {
            if r.is_empty() {
                return Err(CredentialError::Empty("empty_recovery_key"));
            }
            return Ok(Credential::RecoveryKey(r.clone()));
        }
        let pass = read_secret_source(
            self.passphrase_file.as_deref(),
            self.passphrase_stdin,
            "SUBVAULT_PASSPHRASE_FILE",
            "SUBVAULT_PASSPHRASE",
            "empty_passphrase",
        )?;
        let recovery = read_secret_source(
            self.recovery_key_file.as_deref(),
            self.recovery_key_stdin,
            "SUBVAULT_RECOVERY_KEY_FILE",
            "SUBVAULT_RECOVERY_KEY",
            "empty_recovery_key",
        )?;
        match (pass, recovery) {
            (Some(_), Some(_)) => Err(CredentialError::Conflict),
            (Some(p), None) => Ok(Credential::Passphrase(p)),
            (None, Some(r)) => Ok(Credential::RecoveryKey(r)),
            (None, None) => Ok(Credential::None),
        }
    }

    pub fn resolve_passphrase_only(&self) -> Result<Option<Zeroizing<String>>, CredentialError> {
        if self.recovery_key_file.is_some()
            || self.recovery_key_stdin
            || self.direct_recovery_key.is_some()
        {
            return Err(CredentialError::Conflict);
        }
        if let Some(ref p) = self.direct_passphrase {
            if p.is_empty() {
                return Err(CredentialError::Empty("empty_passphrase"));
            }
            return Ok(Some(p.clone()));
        }
        read_secret_source(
            self.passphrase_file.as_deref(),
            self.passphrase_stdin,
            "SUBVAULT_PASSPHRASE_FILE",
            "SUBVAULT_PASSPHRASE",
            "empty_passphrase",
        )
    }
}

impl NewPassphraseInput {
    pub fn resolve(&self) -> Result<Option<Zeroizing<String>>, CredentialError> {
        if let Some(ref p) = self.direct {
            if p.is_empty() {
                return Err(CredentialError::Empty("empty_new_passphrase"));
            }
            return Ok(Some(p.clone()));
        }
        read_secret_source(
            self.file.as_deref(),
            self.stdin,
            "SUBVAULT_NEW_PASSPHRASE_FILE",
            "SUBVAULT_NEW_PASSPHRASE",
            "empty_new_passphrase",
        )
    }
}

fn read_secret_source(
    cli_file: Option<&Path>,
    cli_stdin: bool,
    env_file: &str,
    env_val: &str,
    empty_reason: &'static str,
) -> Result<Option<Zeroizing<String>>, CredentialError> {
    if cli_file.is_some() && cli_stdin {
        return Err(CredentialError::Conflict);
    }
    let raw = if let Some(path) = cli_file {
        Some(read_first_line_file(path)?)
    } else if cli_stdin {
        Some(read_first_line_stdin()?)
    } else if let Ok(path) = std::env::var(env_file) {
        if path.is_empty() {
            return Err(CredentialError::Empty(empty_reason));
        }
        Some(read_first_line_file(Path::new(&path))?)
    } else if let Ok(val) = std::env::var(env_val) {
        Some(Zeroizing::new(strip_trailing_newline(val)))
    } else {
        None
    };
    if let Some(secret) = raw {
        if secret.is_empty() {
            return Err(CredentialError::Empty(empty_reason));
        }
        Ok(Some(secret))
    } else {
        Ok(None)
    }
}

fn read_first_line_file(path: &Path) -> Result<Zeroizing<String>, CredentialError> {
    let text = Zeroizing::new(fs::read_to_string(path).map_err(CredentialError::Io)?);
    let first = text.lines().next().unwrap_or("");
    Ok(Zeroizing::new(strip_trailing_newline(first.to_string())))
}

fn read_first_line_stdin() -> Result<Zeroizing<String>, CredentialError> {
    let mut line = String::new();
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    handle.read_line(&mut line).map_err(CredentialError::Io)?;
    Ok(Zeroizing::new(strip_trailing_newline(line)))
}

fn strip_trailing_newline(mut s: String) -> String {
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    s
}

#[derive(Clone, Debug)]
pub struct KeySlotRecord {
    pub slot_kind: String,
    pub kdf: String,
    pub kdf_params_json: String,
    pub salt_b64: String,
    pub nonce_b64: String,
    pub wrapped_dek_b64: String,
    pub updated_at: i64,
}

#[derive(Clone)]
pub struct VaultKeys {
    vault_id: String,
    dek: Zeroizing<[u8; 32]>,
    object_key: Zeroizing<[u8; 32]>,
    field_key: Zeroizing<[u8; 32]>,
    journal_key: Zeroizing<[u8; 32]>,
    dedup_key: Zeroizing<[u8; 32]>,
}

impl VaultKeys {
    pub fn from_dek(vault_id: &str, dek: Zeroizing<[u8; 32]>) -> Result<Self, CryptoError> {
        let hk = Hkdf::<Sha256>::new(Some(vault_id.as_bytes()), dek.as_ref());
        let mut object_key = Zeroizing::new([0u8; 32]);
        let mut field_key = Zeroizing::new([0u8; 32]);
        let mut journal_key = Zeroizing::new([0u8; 32]);
        let mut dedup_key = Zeroizing::new([0u8; 32]);
        hk.expand(b"subvault-v6-object-aead-1", object_key.as_mut())
            .map_err(|_| CryptoError::Kdf("hkdf object_key".into()))?;
        hk.expand(b"subvault-v6-db-field-aead-1", field_key.as_mut())
            .map_err(|_| CryptoError::Kdf("hkdf field_key".into()))?;
        hk.expand(b"subvault-v6-journal-aead-1", journal_key.as_mut())
            .map_err(|_| CryptoError::Kdf("hkdf journal_key".into()))?;
        hk.expand(b"subvault-v6-dedup-hmac-1", dedup_key.as_mut())
            .map_err(|_| CryptoError::Kdf("hkdf dedup_key".into()))?;
        Ok(Self {
            vault_id: vault_id.to_string(),
            dek,
            object_key,
            field_key,
            journal_key,
            dedup_key,
        })
    }

    pub fn dek(&self) -> &[u8; 32] {
        &self.dek
    }

    pub fn vault_id(&self) -> &str {
        &self.vault_id
    }
}

#[derive(Clone, Default)]
pub enum VaultCipher {
    #[default]
    Plain,
    Aead(Box<VaultKeys>),
}

impl VaultCipher {
    pub fn is_encrypted(&self) -> bool {
        matches!(self, VaultCipher::Aead(_))
    }

    pub fn keys(&self) -> Option<&VaultKeys> {
        match self {
            VaultCipher::Plain => None,
            VaultCipher::Aead(keys) => Some(keys),
        }
    }

    pub fn dedup_key(&self, domain: &str, material: &str) -> String {
        match self {
            VaultCipher::Plain => material.to_string(),
            VaultCipher::Aead(keys) => {
                let mut mac = <HmacSha256 as Mac>::new_from_slice(keys.dedup_key.as_ref())
                    .expect("HMAC accepts any key length");
                mac.update(domain.as_bytes());
                mac.update(b"\0");
                mac.update(material.as_bytes());
                format!("hmac:{}", hex_encode(&mac.finalize().into_bytes()))
            }
        }
    }

    pub fn encrypt_field(
        &self,
        table: &str,
        row_id: &str,
        column: &str,
        plaintext: &str,
    ) -> Result<String, CryptoError> {
        let VaultCipher::Aead(keys) = self else {
            return Ok(plaintext.to_string());
        };
        let mut nonce_bytes = [0u8; 24];
        fill_random(&mut nonce_bytes).map_err(CryptoError::Rng)?;
        let cipher = XChaCha20Poly1305::new(keys.field_key.as_ref().into());
        let aad = field_aad(&keys.vault_id, table, row_id, column);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Tampered("field_encrypt"))?;
        let mut combined = Vec::with_capacity(24 + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        Ok(format!("{FIELD_PREFIX}{}", B64.encode(combined)))
    }

    pub fn decrypt_field(
        &self,
        table: &str,
        row_id: &str,
        column: &str,
        stored: &str,
    ) -> Result<String, CryptoError> {
        let VaultCipher::Aead(keys) = self else {
            return Ok(stored.to_string());
        };
        let Some(encoded) = stored.strip_prefix(FIELD_PREFIX) else {
            return Err(CryptoError::Tampered("missing_enc_prefix"));
        };
        let combined = B64
            .decode(encoded)
            .map_err(|_| CryptoError::Tampered("bad_field_base64"))?;
        if combined.len() < 24 + CHUNK_TAG_SIZE {
            return Err(CryptoError::Tampered("short_field_ciphertext"));
        }
        let (nonce_bytes, ciphertext) = combined.split_at(24);
        let cipher = XChaCha20Poly1305::new(keys.field_key.as_ref().into());
        let aad = field_aad(&keys.vault_id, table, row_id, column);
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Tampered("field_auth_failed"))?;
        String::from_utf8(plaintext).map_err(|_| CryptoError::Tampered("field_non_utf8"))
    }

    pub fn encrypt_opt_field(
        &self,
        table: &str,
        row_id: &str,
        column: &str,
        plaintext: Option<&str>,
    ) -> Result<Option<String>, CryptoError> {
        match plaintext {
            Some(text) => self.encrypt_field(table, row_id, column, text).map(Some),
            None => Ok(None),
        }
    }

    pub fn decrypt_opt_field(
        &self,
        table: &str,
        row_id: &str,
        column: &str,
        stored: Option<&str>,
    ) -> Result<Option<String>, CryptoError> {
        match stored {
            Some(text) => self.decrypt_field(table, row_id, column, text).map(Some),
            None => Ok(None),
        }
    }

    pub fn encrypt_journal(
        &self,
        command_id: &str,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let VaultCipher::Aead(keys) = self else {
            return Ok(plaintext.to_vec());
        };
        let mut nonce_bytes = [0u8; 24];
        fill_random(&mut nonce_bytes).map_err(CryptoError::Rng)?;
        let cipher = XChaCha20Poly1305::new(keys.journal_key.as_ref().into());
        let aad = journal_aad(&keys.vault_id, command_id);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Tampered("journal_encrypt"))?;
        let mut out = Vec::with_capacity(JOURNAL_MAGIC.len() + 24 + ciphertext.len());
        out.extend_from_slice(JOURNAL_MAGIC);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn decrypt_journal(&self, command_id: &str, stored: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let VaultCipher::Aead(keys) = self else {
            return Ok(stored.to_vec());
        };
        if stored.len() < JOURNAL_MAGIC.len() + 24 + CHUNK_TAG_SIZE
            || &stored[..JOURNAL_MAGIC.len()] != JOURNAL_MAGIC
        {
            return Err(CryptoError::Tampered("journal_header"));
        }
        let nonce_bytes = &stored[JOURNAL_MAGIC.len()..JOURNAL_MAGIC.len() + 24];
        let ciphertext = &stored[JOURNAL_MAGIC.len() + 24..];
        let cipher = XChaCha20Poly1305::new(keys.journal_key.as_ref().into());
        let aad = journal_aad(&keys.vault_id, command_id);
        cipher
            .decrypt(
                XNonce::from_slice(nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Tampered("journal_auth_failed"))
    }

    pub fn encrypt_object_bytes(
        &self,
        object_id: &str,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let VaultCipher::Aead(keys) = self else {
            return Ok(plaintext.to_vec());
        };
        let mut out = Vec::with_capacity(plaintext.len() + 64);
        encrypt_stream(keys, object_id, plaintext, &mut out)?;
        Ok(out)
    }

    pub fn encrypt_object_stream<R: Read, W: Write>(
        &self,
        object_id: &str,
        mut reader: R,
        mut writer: W,
    ) -> Result<(String, u64), CryptoError> {
        match self {
            VaultCipher::Plain => {
                let mut hasher = Sha256::new();
                let mut total = 0u64;
                let mut buf = [0u8; CHUNK_PLAIN_SIZE];
                loop {
                    let n = reader.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    writer.write_all(&buf[..n])?;
                    total += n as u64;
                }
                Ok((hex_encode(&hasher.finalize()), total))
            }
            VaultCipher::Aead(keys) => encrypt_stream(keys, object_id, reader, writer),
        }
    }

    pub fn verify_object_file(
        &self,
        object_id: &str,
        path: &Path,
    ) -> Result<(String, u64), CryptoError> {
        match self {
            VaultCipher::Plain => crate::fsutil::sha256_file(path).map_err(CryptoError::Io),
            VaultCipher::Aead(keys) => {
                let file = File::open(path)?;
                let mut sink = io::sink();
                decrypt_stream(keys, object_id, file, &mut sink)
            }
        }
    }

    pub fn decrypt_object_to_vec(
        &self,
        object_id: &str,
        path: &Path,
    ) -> Result<Vec<u8>, CryptoError> {
        match self {
            VaultCipher::Plain => fs::read(path).map_err(CryptoError::Io),
            VaultCipher::Aead(keys) => {
                let file = File::open(path)?;
                let mut out = Vec::new();
                decrypt_stream(keys, object_id, file, &mut out)?;
                Ok(out)
            }
        }
    }

    pub fn copy_decrypted_object<W: Write>(
        &self,
        object_id: &str,
        src: &Path,
        mut dst: W,
    ) -> Result<(String, u64), CryptoError> {
        match self {
            VaultCipher::Plain => {
                let mut file = File::open(src)?;
                let mut hasher = Sha256::new();
                let mut total = 0u64;
                let mut buf = [0u8; CHUNK_PLAIN_SIZE];
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    dst.write_all(&buf[..n])?;
                    total += n as u64;
                }
                Ok((hex_encode(&hasher.finalize()), total))
            }
            VaultCipher::Aead(keys) => {
                let file = File::open(src)?;
                decrypt_stream(keys, object_id, file, dst)
            }
        }
    }
}

fn encrypt_stream<R: Read, W: Write>(
    keys: &VaultKeys,
    object_id: &str,
    mut reader: R,
    mut writer: W,
) -> Result<(String, u64), CryptoError> {
    let mut base_nonce = [0u8; 24];
    fill_random(&mut base_nonce).map_err(CryptoError::Rng)?;
    writer.write_all(OBJECT_MAGIC)?;
    writer.write_all(&base_nonce)?;

    let cipher = XChaCha20Poly1305::new(keys.object_key.as_ref().into());
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut chunk_idx = 0u64;
    let mut cur = Vec::with_capacity(CHUNK_PLAIN_SIZE);
    let mut next = Vec::with_capacity(CHUNK_PLAIN_SIZE);

    fill_chunk(&mut reader, &mut cur)?;
    loop {
        next.clear();
        fill_chunk(&mut reader, &mut next)?;
        let is_final = next.is_empty();
        let flags = if is_final { FLAG_FINAL } else { FLAG_MORE };
        hasher.update(&cur);
        total += cur.len() as u64;

        let nonce = chunk_nonce(&base_nonce, chunk_idx);
        let aad = object_chunk_aad(&keys.vault_id, object_id, chunk_idx, flags);
        let ct = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &cur,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Tampered("object_chunk_encrypt"))?;
        let ct_len =
            u32::try_from(ct.len()).map_err(|_| CryptoError::InvalidFormat("chunk_len"))?;
        writer.write_all(&ct_len.to_be_bytes())?;
        writer.write_all(&[flags])?;
        writer.write_all(&ct)?;

        if is_final {
            break;
        }
        std::mem::swap(&mut cur, &mut next);
        chunk_idx = chunk_idx
            .checked_add(1)
            .ok_or(CryptoError::InvalidFormat("chunk_overflow"))?;
    }
    Ok((hex_encode(&hasher.finalize()), total))
}

fn fill_chunk<R: Read>(reader: &mut R, buf: &mut Vec<u8>) -> io::Result<()> {
    buf.clear();
    let mut tmp = [0u8; 8192];
    while buf.len() < CHUNK_PLAIN_SIZE {
        let want = (CHUNK_PLAIN_SIZE - buf.len()).min(tmp.len());
        let n = reader.read(&mut tmp[..want])?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    Ok(())
}

fn decrypt_stream<R: Read, W: Write>(
    keys: &VaultKeys,
    object_id: &str,
    mut reader: R,
    mut writer: W,
) -> Result<(String, u64), CryptoError> {
    let mut header = [0u8; 8 + 24];
    read_exact_or_tampered(&mut reader, &mut header, "object_header")?;
    if &header[..8] != OBJECT_MAGIC {
        return Err(CryptoError::Tampered("object_magic_mismatch"));
    }
    let mut base_nonce = [0u8; 24];
    base_nonce.copy_from_slice(&header[8..32]);

    let cipher = XChaCha20Poly1305::new(keys.object_key.as_ref().into());
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut chunk_idx = 0u64;
    let max_cipher = CHUNK_PLAIN_SIZE + CHUNK_TAG_SIZE;

    loop {
        let mut prefix = [0u8; 5];
        read_exact_or_tampered(&mut reader, &mut prefix, "object_chunk_prefix")?;
        let ct_len = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
        let flags = prefix[4];
        if (flags != FLAG_MORE && flags != FLAG_FINAL)
            || ct_len < CHUNK_TAG_SIZE
            || ct_len > max_cipher
        {
            return Err(CryptoError::Tampered("object_chunk_bounds"));
        }
        let mut ct = vec![0u8; ct_len];
        read_exact_or_tampered(&mut reader, &mut ct, "object_chunk_body")?;
        let nonce = chunk_nonce(&base_nonce, chunk_idx);
        let aad = object_chunk_aad(&keys.vault_id, object_id, chunk_idx, flags);
        let plain = cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ct,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Tampered("object_chunk_auth"))?;
        hasher.update(&plain);
        writer.write_all(&plain)?;
        total += plain.len() as u64;
        if flags == FLAG_FINAL {
            let mut trailing = [0u8; 1];
            if reader.read(&mut trailing)? != 0 {
                return Err(CryptoError::Tampered("object_trailing_bytes"));
            }
            break;
        }
        chunk_idx = chunk_idx
            .checked_add(1)
            .ok_or(CryptoError::Tampered("chunk_overflow"))?;
    }
    Ok((hex_encode(&hasher.finalize()), total))
}

fn read_exact_or_tampered<R: Read>(
    reader: &mut R,
    buf: &mut [u8],
    label: &'static str,
) -> Result<(), CryptoError> {
    match reader.read_exact(buf) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Err(CryptoError::Tampered(label)),
        Err(err) => Err(CryptoError::Io(err)),
    }
}

fn chunk_nonce(base_nonce: &[u8; 24], chunk_idx: u64) -> [u8; 24] {
    let mut nonce = *base_nonce;
    let idx = chunk_idx.to_be_bytes();
    for i in 0..8 {
        nonce[16 + i] ^= idx[i];
    }
    nonce
}

fn field_aad(vault_id: &str, table: &str, row_id: &str, column: &str) -> Vec<u8> {
    let mut aad =
        Vec::with_capacity(24 + vault_id.len() + table.len() + row_id.len() + column.len());
    aad.extend_from_slice(b"subvault-v6-field\0");
    aad.extend_from_slice(vault_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(table.as_bytes());
    aad.push(0);
    aad.extend_from_slice(row_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(column.as_bytes());
    aad
}

fn journal_aad(vault_id: &str, command_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(24 + vault_id.len() + command_id.len());
    aad.extend_from_slice(b"subvault-v6-journal\0");
    aad.extend_from_slice(vault_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(command_id.as_bytes());
    aad
}

fn object_chunk_aad(vault_id: &str, object_id: &str, chunk_idx: u64, flags: u8) -> Vec<u8> {
    let mut aad = Vec::with_capacity(32 + vault_id.len() + object_id.len());
    aad.extend_from_slice(b"subvault-v6-obj-chunk\0");
    aad.extend_from_slice(vault_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(object_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(&chunk_idx.to_be_bytes());
    aad.push(flags);
    aad
}

fn slot_aad(vault_id: &str, slot_kind: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(24 + vault_id.len() + slot_kind.len());
    aad.extend_from_slice(b"subvault-v6-slot\0");
    aad.extend_from_slice(vault_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(slot_kind.as_bytes());
    aad
}

pub fn generate_dek() -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let mut dek = Zeroizing::new([0u8; 32]);
    fill_random(dek.as_mut()).map_err(CryptoError::Rng)?;
    Ok(dek)
}

pub fn generate_recovery_key() -> Result<(Zeroizing<[u8; 32]>, Zeroizing<String>), CryptoError> {
    let mut raw = Zeroizing::new([0u8; 32]);
    fill_random(raw.as_mut()).map_err(CryptoError::Rng)?;
    let formatted = encode_recovery_key(&raw);
    Ok((raw, formatted))
}

pub fn encode_recovery_key(raw: &[u8; 32]) -> Zeroizing<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"subvault-recovery-key-v1\0");
    hasher.update(raw);
    let digest = hasher.finalize();

    let mut payload = Zeroizing::new([0u8; 35]);
    payload[..32].copy_from_slice(raw);
    payload[32..35].copy_from_slice(&digest[..3]);

    let encoded = Zeroizing::new(base32_encode(payload.as_ref()));
    let mut out = String::with_capacity(5 + encoded.len() + 14);
    out.push_str("SVRK");
    for chunk in encoded.as_bytes().chunks(4) {
        out.push('-');
        out.push_str(std::str::from_utf8(chunk).expect("ascii base32"));
    }
    Zeroizing::new(out)
}

pub fn decode_recovery_key(text: &str) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let trimmed = text.trim();
    let stripped = trimmed
        .strip_prefix("SVRK-")
        .or_else(|| trimmed.strip_prefix("svrk-"))
        .ok_or(CryptoError::InvalidFormat("recovery_key_prefix"))?;
    let clean = Zeroizing::new(
        stripped
            .chars()
            .filter(|c| *c != '-' && !c.is_ascii_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect::<String>(),
    );
    if clean.len() != 56 {
        return Err(CryptoError::InvalidFormat("recovery_key_length"));
    }
    let decoded = base32_decode(&clean)?;
    if decoded.len() != 35 {
        return Err(CryptoError::InvalidFormat("recovery_key_decoded_len"));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"subvault-recovery-key-v1\0");
    hasher.update(&decoded[..32]);
    let digest = hasher.finalize();
    let mut diff = 0u8;
    for i in 0..3 {
        diff |= decoded[32 + i] ^ digest[i];
    }
    if diff != 0 {
        return Err(CryptoError::InvalidFormat("recovery_key_checksum"));
    }
    let mut raw = Zeroizing::new([0u8; 32]);
    raw.copy_from_slice(&decoded[..32]);
    Ok(raw)
}

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((acc >> bits) & 0x1F) as usize;
            out.push(BASE32_ALPHABET[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((acc << (5 - bits)) & 0x1F) as usize;
        out.push(BASE32_ALPHABET[idx] as char);
    }
    out
}

fn base32_decode(text: &str) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let mut out = Zeroizing::new(Vec::with_capacity((text.len() * 5) / 8));
    let mut acc = 0u32;
    let mut bits = 0u32;
    for b in text.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'2'..=b'7' => (b - b'2') + 26,
            _ => return Err(CryptoError::InvalidFormat("recovery_key_alphabet")),
        };
        acc = (acc << 5) | u32::from(val);
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Ok(out)
}

pub fn wrap_dek_with_passphrase(
    vault_id: &str,
    dek: &[u8; 32],
    passphrase: &str,
    updated_at: i64,
) -> Result<KeySlotRecord, CryptoError> {
    let mut salt = [0u8; 16];
    fill_random(&mut salt).map_err(CryptoError::Rng)?;
    let kek = derive_argon2_kek(
        passphrase,
        &salt,
        DEFAULT_ARGON2_M_COST,
        DEFAULT_ARGON2_T_COST,
        DEFAULT_ARGON2_P_COST,
    )?;
    let mut nonce = [0u8; 24];
    fill_random(&mut nonce).map_err(CryptoError::Rng)?;
    let cipher = XChaCha20Poly1305::new(kek.as_ref().into());
    let aad = slot_aad(vault_id, "passphrase");
    let wrapped = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: dek,
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::Tampered("wrap_passphrase"))?;
    Ok(KeySlotRecord {
        slot_kind: "passphrase".into(),
        kdf: "argon2id-v19".into(),
        kdf_params_json: json!({
            "m_cost": DEFAULT_ARGON2_M_COST,
            "t_cost": DEFAULT_ARGON2_T_COST,
            "p_cost": DEFAULT_ARGON2_P_COST
        })
        .to_string(),
        salt_b64: B64.encode(salt),
        nonce_b64: B64.encode(nonce),
        wrapped_dek_b64: B64.encode(wrapped),
        updated_at,
    })
}

pub fn wrap_dek_with_recovery(
    vault_id: &str,
    dek: &[u8; 32],
    recovery_raw: &[u8; 32],
    updated_at: i64,
) -> Result<KeySlotRecord, CryptoError> {
    let mut salt = [0u8; 16];
    fill_random(&mut salt).map_err(CryptoError::Rng)?;
    let kek = derive_recovery_kek(recovery_raw, &salt)?;
    let mut nonce = [0u8; 24];
    fill_random(&mut nonce).map_err(CryptoError::Rng)?;
    let cipher = XChaCha20Poly1305::new(kek.as_ref().into());
    let aad = slot_aad(vault_id, "recovery");
    let wrapped = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: dek,
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::Tampered("wrap_recovery"))?;
    Ok(KeySlotRecord {
        slot_kind: "recovery".into(),
        kdf: "hkdf-sha256-v1".into(),
        kdf_params_json: json!({ "info": "subvault-v6-recovery-kek-1" }).to_string(),
        salt_b64: B64.encode(salt),
        nonce_b64: B64.encode(nonce),
        wrapped_dek_b64: B64.encode(wrapped),
        updated_at,
    })
}

pub fn unwrap_dek_with_passphrase(
    vault_id: &str,
    slot: &KeySlotRecord,
    passphrase: &str,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    if slot.slot_kind != "passphrase" || slot.kdf != "argon2id-v19" {
        return Err(CryptoError::InvalidFormat("passphrase_slot_kdf"));
    }
    let params: Value = serde_json::from_str(&slot.kdf_params_json)
        .map_err(|_| CryptoError::InvalidFormat("kdf_params_json"))?;
    let m_cost = params
        .get("m_cost")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(CryptoError::InvalidFormat("m_cost"))?;
    let t_cost = params
        .get("t_cost")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(CryptoError::InvalidFormat("t_cost"))?;
    let p_cost = params
        .get("p_cost")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(CryptoError::InvalidFormat("p_cost"))?;
    if !(MIN_ARGON2_M_COST..=MAX_ARGON2_M_COST).contains(&m_cost)
        || !(MIN_ARGON2_T_COST..=MAX_ARGON2_T_COST).contains(&t_cost)
        || !(MIN_ARGON2_P_COST..=MAX_ARGON2_P_COST).contains(&p_cost)
    {
        return Err(CryptoError::InvalidFormat("kdf_params_out_of_bounds"));
    }
    let salt = B64
        .decode(&slot.salt_b64)
        .map_err(|_| CryptoError::InvalidFormat("salt_b64"))?;
    if !(MIN_ARGON2_SALT_LEN..=MAX_ARGON2_SALT_LEN).contains(&salt.len()) {
        return Err(CryptoError::InvalidFormat("salt_len"));
    }
    let kek = derive_argon2_kek(passphrase, &salt, m_cost, t_cost, p_cost)?;
    unwrap_slot_dek(vault_id, "passphrase", slot, &kek)
}

pub fn unwrap_dek_with_recovery(
    vault_id: &str,
    slot: &KeySlotRecord,
    recovery_text: &str,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    if slot.slot_kind != "recovery" || slot.kdf != "hkdf-sha256-v1" {
        return Err(CryptoError::InvalidFormat("recovery_slot_kdf"));
    }
    let recovery_raw = decode_recovery_key(recovery_text).map_err(|_| CryptoError::AuthFailed)?;
    let salt = B64
        .decode(&slot.salt_b64)
        .map_err(|_| CryptoError::InvalidFormat("salt_b64"))?;
    let kek = derive_recovery_kek(&recovery_raw, &salt)?;
    unwrap_slot_dek(vault_id, "recovery", slot, &kek)
}

fn unwrap_slot_dek(
    vault_id: &str,
    slot_kind: &str,
    slot: &KeySlotRecord,
    kek: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let nonce = B64
        .decode(&slot.nonce_b64)
        .map_err(|_| CryptoError::InvalidFormat("nonce_b64"))?;
    if nonce.len() != 24 {
        return Err(CryptoError::InvalidFormat("nonce_len"));
    }
    let wrapped = B64
        .decode(&slot.wrapped_dek_b64)
        .map_err(|_| CryptoError::InvalidFormat("wrapped_dek_b64"))?;
    let cipher = XChaCha20Poly1305::new(kek.into());
    let aad = slot_aad(vault_id, slot_kind);
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &wrapped,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::AuthFailed)?,
    );
    if plain.len() != 32 {
        return Err(CryptoError::InvalidFormat("dek_len"));
    }
    let mut dek = Zeroizing::new([0u8; 32]);
    dek.copy_from_slice(&plain);
    Ok(dek)
}

fn derive_argon2_kek(
    passphrase: &str,
    salt: &[u8],
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let params = Params::new(m_cost, t_cost, p_cost, Some(32))
        .map_err(|err| CryptoError::Kdf(err.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, out.as_mut())
        .map_err(|err| CryptoError::Kdf(err.to_string()))?;
    Ok(out)
}

fn derive_recovery_kek(raw: &[u8; 32], salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let hk = Hkdf::<Sha256>::new(Some(salt), raw);
    let mut out = Zeroizing::new([0u8; 32]);
    hk.expand(b"subvault-v6-recovery-kek-1", out.as_mut())
        .map_err(|_| CryptoError::Kdf("hkdf recovery kek".into()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_key_round_trip_and_checksum_reject_corruption() {
        let (raw, formatted) = generate_recovery_key().expect("recovery key");
        assert!(formatted.starts_with("SVRK-"));
        let decoded = decode_recovery_key(&formatted).expect("decode clean");
        assert_eq!(decoded.as_ref(), raw.as_ref());

        // Lowercase and spaces are accepted
        let relaxed = formatted.to_lowercase().replace('-', " ");
        assert_eq!(
            decode_recovery_key(&format!("svrk-{}", &relaxed[5..]))
                .expect("decode relaxed")
                .as_ref(),
            raw.as_ref()
        );

        // Single flipped character must fail checksum
        let mut chars: Vec<char> = formatted.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == 'A' { 'B' } else { 'A' };
        let bad: String = chars.into_iter().collect();
        assert!(decode_recovery_key(&bad).is_err());
    }

    #[test]
    fn passphrase_and_recovery_slots_unwrap_same_dek_and_reject_wrong_secret() {
        let dek = generate_dek().expect("dek");
        let (rec_raw, rec_str) = generate_recovery_key().expect("recovery");
        let pass_slot =
            wrap_dek_with_passphrase("vlt_test", &dek, "correct horse battery staple", 1)
                .expect("pass slot");
        let rec_slot = wrap_dek_with_recovery("vlt_test", &dek, &rec_raw, 1).expect("rec slot");

        let d1 = unwrap_dek_with_passphrase("vlt_test", &pass_slot, "correct horse battery staple")
            .expect("unwrap pass");
        let d2 = unwrap_dek_with_recovery("vlt_test", &rec_slot, &rec_str).expect("unwrap rec");
        assert_eq!(d1.as_ref(), dek.as_ref());
        assert_eq!(d2.as_ref(), dek.as_ref());

        assert!(matches!(
            unwrap_dek_with_passphrase("vlt_test", &pass_slot, "wrong pass"),
            Err(CryptoError::AuthFailed)
        ));
        // Wrong vault_id AAD fails authentication even with right passphrase
        assert!(matches!(
            unwrap_dek_with_passphrase("vlt_other", &pass_slot, "correct horse battery staple"),
            Err(CryptoError::AuthFailed)
        ));

        // Malicious out-of-bounds Argon2id parameters must be rejected before KDF allocation
        let mut oom_slot = pass_slot.clone();
        oom_slot.kdf_params_json =
            json!({ "m_cost": u32::MAX, "t_cost": 3, "p_cost": 1 }).to_string();
        assert!(matches!(
            unwrap_dek_with_passphrase("vlt_test", &oom_slot, "correct horse battery staple"),
            Err(CryptoError::InvalidFormat("kdf_params_out_of_bounds"))
        ));

        let mut weak_slot = pass_slot.clone();
        weak_slot.kdf_params_json = json!({ "m_cost": 8, "t_cost": 1, "p_cost": 1 }).to_string();
        assert!(matches!(
            unwrap_dek_with_passphrase("vlt_test", &weak_slot, "correct horse battery staple"),
            Err(CryptoError::InvalidFormat("kdf_params_out_of_bounds"))
        ));
    }

    #[test]
    fn field_and_chunked_object_aead_bind_aad_and_catch_tamper_truncation_and_swap() {
        let dek = generate_dek().expect("dek");
        let keys = VaultKeys::from_dek("vlt_a", dek).expect("keys");
        let cipher = VaultCipher::Aead(Box::new(keys));

        // Field AEAD binds table, row_id, column
        let ct = cipher
            .encrypt_field("facts", "fact_1", "value_json", r#"{"amount":"15.99"}"#)
            .expect("encrypt field");
        assert!(ct.starts_with(FIELD_PREFIX));
        assert!(!ct.contains("15.99"));
        assert_eq!(
            cipher
                .decrypt_field("facts", "fact_1", "value_json", &ct)
                .expect("decrypt"),
            r#"{"amount":"15.99"}"#
        );
        // Cross-row swap rejected!
        assert!(cipher
            .decrypt_field("facts", "fact_2", "value_json", &ct)
            .is_err());
        // Cross-column swap rejected!
        assert!(cipher
            .decrypt_field("facts", "fact_1", "other_col", &ct)
            .is_err());

        // Multi-chunk object (> 64 KiB) + 0-byte empty object
        let big_plain: Vec<u8> = (0..(CHUNK_PLAIN_SIZE * 2 + 123))
            .map(|i| (i % 251) as u8)
            .collect();
        let big_ct = cipher
            .encrypt_object_bytes("obj_1", &big_plain)
            .expect("encrypt big");
        assert!(big_ct.starts_with(OBJECT_MAGIC));

        let mut recovered = Vec::new();
        let (_, len) = decrypt_stream(cipher.keys().unwrap(), "obj_1", &big_ct[..], &mut recovered)
            .expect("decrypt big");
        assert_eq!(len as usize, big_plain.len());
        assert_eq!(recovered, big_plain);

        // Wrong object_id swap rejected!
        let mut sink = Vec::new();
        assert!(decrypt_stream(cipher.keys().unwrap(), "obj_2", &big_ct[..], &mut sink).is_err());

        // Truncation rejected!
        assert!(decrypt_stream(
            cipher.keys().unwrap(),
            "obj_1",
            &big_ct[..big_ct.len() - 10],
            &mut sink
        )
        .is_err());

        // Trailing bytes rejected!
        let mut extra = big_ct.clone();
        extra.push(0x42);
        assert!(decrypt_stream(cipher.keys().unwrap(), "obj_1", &extra[..], &mut sink).is_err());

        // Empty 0-byte object round-trips cleanly
        let empty_ct = cipher
            .encrypt_object_bytes("obj_empty", b"")
            .expect("empty");
        let mut empty_out = Vec::new();
        let (_, empty_len) = decrypt_stream(
            cipher.keys().unwrap(),
            "obj_empty",
            &empty_ct[..],
            &mut empty_out,
        )
        .expect("decrypt empty");
        assert_eq!(empty_len, 0);
        assert!(empty_out.is_empty());
    }
}
