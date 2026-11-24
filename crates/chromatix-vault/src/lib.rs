//! Pure Rust Chromatix Pixel Standard (`.vault.png`) encrypted secret vault.
//!
//! Host-bound security model:
//! - Encrypted `.vault.png` files are stored per tenant on disk.
//! - The host master key lives ONLY in the `EARNEST_VAULT_KEY` environment variable
//!   (never persisted in `.vault.png`, YAML, SQLite, or Git).
//! - Per-tenant keys are derived via `HMAC-SHA256(host_key, tenant_id)`.
//! - Payloads are compressed (zlib), encrypted with a ChaCha20 keystream derived
//!   from `(tenant_key, nonce)`, and authenticated with `HMAC-SHA256`.

mod png;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use hmac::{Hmac, Mac};
use rand::RngCore;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

type HmacSha256 = Hmac<Sha256>;

/// Host environment variable holding the machine's master secret key.
pub const HOST_KEY_ENV: &str = "SECRET_VAULT_KEY";
/// Legacy/alternative host key environment variable.
pub const ALT_HOST_KEY_ENV: &str = "EARNEST_VAULT_KEY";
/// Optional override for the directory holding `.vault.png` files.
pub const VAULTS_DIR_ENV: &str = "EARNEST_VAULTS_DIR";

const CPS_MAGIC: [u8; 3] = [0xFF, 0x43, 0x53]; // "CPS"
const FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 16;
const TAG_LEN: usize = 32;
// Header: 3 (magic) + 1 (ver) + 16 (nonce) + 4 (payload_len) = 24 bytes (8 RGB pixels)
const HEADER_LEN: usize = 24;

#[derive(Debug, Clone)]
pub struct TenantVault {
    vaults_dir: PathBuf,
    host_key: String,
}

/// Resolves the default directory for `.vault.png` files:
/// `EARNEST_VAULTS_DIR` -> `%APPDATA%/Earnest/vaults` or `~/.earnest/vaults` -> `.earnest/vaults`.
pub fn resolve_vaults_dir() -> PathBuf {
    if let Ok(env_dir) = std::env::var(VAULTS_DIR_ENV) {
        let t = env_dir.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("Earnest").join("vaults");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".earnest").join("vaults");
    }
    PathBuf::from(".earnest").join("vaults")
}

impl TenantVault {
    /// Creates a `TenantVault` bound to an explicit host master key.
    pub fn new(vaults_dir: impl AsRef<Path>, host_key: impl Into<String>) -> Result<Self, String> {
        let hk = host_key.into();
        if hk.trim().is_empty() {
            return Err(format!("Clé hôte vide : définissez {}", HOST_KEY_ENV));
        }
        let dir = vaults_dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(|e| format!("Création vault dir '{}': {}", dir.display(), e))?;
        Ok(Self {
            vaults_dir: dir,
            host_key: hk,
        })
    }

    /// Creates a `TenantVault` reading the host master key from `SECRET_VAULT_KEY` (or `EARNEST_VAULT_KEY`).
    pub fn from_env(vaults_dir: impl AsRef<Path>) -> Result<Self, String> {
        let hk = std::env::var(HOST_KEY_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var(ALT_HOST_KEY_ENV).ok().filter(|s| !s.trim().is_empty()))
            .ok_or_else(|| {
                format!(
                    "Variable d'environnement '{}' absente sur cette machine : déchiffrement impossible",
                    HOST_KEY_ENV
                )
            })?;
        Self::new(vaults_dir, hk)
    }

    pub fn vault_path(&self, tenant_id: &str) -> PathBuf {
        let safe: String = tenant_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let name = if safe.is_empty() { "default" } else { &safe };
        self.vaults_dir.join(format!("{}.vault.png", name))
    }

    fn derive_tenant_key(&self, tenant_id: &str) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(self.host_key.as_bytes()).expect("HMAC accepts any key");
        mac.update(tenant_id.as_bytes());
        mac.finalize().into_bytes().into()
    }

    fn stream_xor(tenant_key: &[u8; 32], nonce: &[u8; NONCE_LEN], data: &[u8]) -> Vec<u8> {
        let mut h = Sha256::new();
        h.update(tenant_key);
        h.update(nonce);
        let seed: [u8; 32] = h.finalize().into();
        let mut rng = ChaCha20Rng::from_seed(seed);
        let mut keystream = vec![0u8; data.len()];
        rng.fill_bytes(&mut keystream);
        data.iter().zip(keystream.iter()).map(|(a, b)| a ^ b).collect()
    }

    fn encode_packet(&self, tenant_id: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let tenant_key = self.derive_tenant_key(tenant_id);

        let mut zlib = ZlibEncoder::new(Vec::new(), Compression::best());
        zlib.write_all(plaintext).map_err(|e| e.to_string())?;
        let compressed = zlib.finish().map_err(|e| e.to_string())?;

        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ciphertext = Self::stream_xor(&tenant_key, &nonce, &compressed);

        let mut packet = Vec::with_capacity(HEADER_LEN + ciphertext.len() + TAG_LEN);
        packet.extend_from_slice(&CPS_MAGIC);
        packet.push(FORMAT_VERSION);
        packet.extend_from_slice(&nonce);
        packet.extend_from_slice(&(ciphertext.len() as u32).to_be_bytes());
        packet.extend_from_slice(&ciphertext);

        let mut mac = HmacSha256::new_from_slice(&tenant_key).expect("HMAC key");
        mac.update(&packet);
        let tag: [u8; TAG_LEN] = mac.finalize().into_bytes().into();
        packet.extend_from_slice(&tag);

        let total_pixels = (packet.len() + 2) / 3;
        let side = (total_pixels as f64).sqrt().ceil() as usize;
        let mut rgb = vec![0u8; side * side * 3];
        rgb[..packet.len()].copy_from_slice(&packet);
        png::encode_rgb_png(side, &rgb)
    }

    fn decode_packet(&self, tenant_id: &str, png_bytes: &[u8]) -> Result<Vec<u8>, String> {
        let tenant_key = self.derive_tenant_key(tenant_id);
        let (_side, rgb) = png::decode_rgb_png(png_bytes)?;
        if rgb.len() < HEADER_LEN + TAG_LEN {
            return Err("Paquet Chromatix invalide (trop court)".to_string());
        }
        if rgb[0..3] != CPS_MAGIC || rgb[3] != FORMAT_VERSION {
            return Err("Signature CPS Chromatix invalide".to_string());
        }
        let nonce: [u8; NONCE_LEN] = rgb[4..20].try_into().unwrap();
        let ct_len = u32::from_be_bytes(rgb[20..24].try_into().unwrap()) as usize;
        if HEADER_LEN + ct_len + TAG_LEN > rgb.len() {
            return Err("Longueur de payload Chromatix invalide".to_string());
        }
        let body = &rgb[..HEADER_LEN + ct_len];
        let tag = &rgb[HEADER_LEN + ct_len..HEADER_LEN + ct_len + TAG_LEN];

        let mut mac = HmacSha256::new_from_slice(&tenant_key).expect("HMAC key");
        mac.update(body);
        mac.verify_slice(tag)
            .map_err(|_| "Clé hôte invalide ou coffre .vault.png altéré (HMAC mismatch)".to_string())?;

        let compressed = Self::stream_xor(&tenant_key, &nonce, &rgb[HEADER_LEN..HEADER_LEN + ct_len]);
        let mut dec = ZlibDecoder::new(&compressed[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out)
            .map_err(|e| format!("Décompression Chromatix impossible: {}", e))?;
        Ok(out)
    }

    fn load_map(&self, tenant_id: &str) -> Result<BTreeMap<String, String>, String> {
        let path = self.vault_path(tenant_id);
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let bytes = fs::read(&path).map_err(|e| format!("Lecture vault '{}': {}", path.display(), e))?;
        let raw = self.decode_packet(tenant_id, &bytes)?;
        serde_json::from_slice(&raw).map_err(|e| format!("JSON vault invalide: {}", e))
    }

    fn save_map(&self, tenant_id: &str, map: &BTreeMap<String, String>) -> Result<(), String> {
        let raw = serde_json::to_vec_pretty(map).map_err(|e| e.to_string())?;
        let png_bytes = self.encode_packet(tenant_id, &raw)?;
        let path = self.vault_path(tenant_id);
        fs::write(&path, png_bytes).map_err(|e| format!("Écriture vault '{}': {}", path.display(), e))
    }

    pub fn set(&self, tenant_id: &str, key: &str, value: &str) -> Result<(), String> {
        let mut map = self.load_map(tenant_id)?;
        map.insert(key.to_string(), value.to_string());
        self.save_map(tenant_id, &map)
    }

    pub fn get(&self, tenant_id: &str, key: &str) -> Result<Option<String>, String> {
        let map = self.load_map(tenant_id)?;
        Ok(map.get(key).cloned())
    }

    pub fn delete(&self, tenant_id: &str, key: &str) -> Result<bool, String> {
        let mut map = self.load_map(tenant_id)?;
        let removed = map.remove(key).is_some();
        if removed {
            self.save_map(tenant_id, &map)?;
        }
        Ok(removed)
    }

    pub fn list_keys(&self, tenant_id: &str) -> Result<Vec<String>, String> {
        let map = self.load_map(tenant_id)?;
        Ok(map.keys().cloned().collect())
    }

    pub fn load_all(&self, tenant_id: &str) -> Result<HashMap<String, String>, String> {
        let map = self.load_map(tenant_id)?;
        Ok(map.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_png_roundtrip_host_key_isolation_and_tamper_detection() {
        let dir = std::env::temp_dir().join("chromatix_vault_test");
        let _ = fs::remove_dir_all(&dir);

        let vault = TenantVault::new(&dir, "host-secret-key-2026").unwrap();
        vault.set("tenant_a", "smtp_pass", "s3cr3t!").unwrap();
        vault.set("tenant_a", "api_token", "tok_12345").unwrap();
        assert_eq!(vault.get("tenant_a", "smtp_pass").unwrap().as_deref(), Some("s3cr3t!"));
        assert_eq!(vault.list_keys("tenant_a").unwrap(), vec!["api_token", "smtp_pass"]);

        // Different host key must fail HMAC verification
        let wrong_host = TenantVault::new(&dir, "wrong-host-key").unwrap();
        assert!(wrong_host.get("tenant_a", "smtp_pass").is_err());

        // Different tenant on same host must not read tenant_a's vault
        assert_eq!(vault.get("tenant_b", "smtp_pass").unwrap(), None);

        // Tampering with .vault.png must be detected
        let p = vault.vault_path("tenant_a");
        let mut bytes = fs::read(&p).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        fs::write(&p, bytes).unwrap();
        assert!(vault.get("tenant_a", "smtp_pass").is_err());

        let _ = fs::remove_dir_all(&dir);
    }
}
