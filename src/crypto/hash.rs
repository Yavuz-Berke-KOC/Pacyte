// ===================================================================
// PACYTE NEXUS - HASH FONKSİYONLARI
// ===================================================================

use sha2::{Sha256, Sha512, Digest as Sha2Digest};
use sha3::{Sha3_256, Sha3_512, Keccak256, Digest as Sha3Digest};
use blake3::Hasher as Blake3Hasher;

use crate::types::Hash;

// ===================================================================
// SHA2 AİLESİ
// ===================================================================

/// SHA-256 hash hesaplar
pub fn hash_sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// SHA-512 hash hesaplar
pub fn hash_sha512(data: &[u8]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Çift SHA-256 (Bitcoin stili)
pub fn hash256(data: &[u8]) -> [u8; 32] {
    let first = hash_sha256(data);
    hash_sha256(&first)
}

// ===================================================================
// SHA3 AİLESİ (Keccak)
// ===================================================================

/// SHA3-256 hash hesaplar (Pacyte standart hash)
pub fn hash_sha3_256(data: &[u8]) -> Hash {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// SHA3-512 hash hesaplar
pub fn hash_sha3_512(data: &[u8]) -> [u8; 64] {
    let mut hasher = Sha3_512::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Keccak-256 hash hesaplar (Ethereum stili)
pub fn hash_keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(data);
    hasher.finalize().into()
}

// ===================================================================
// BLAKE3 (Yüksek performanslı)
// ===================================================================

/// BLAKE3 hash hesaplar (çok hızlı)
pub fn hash_blake3(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// BLAKE3 keyed hash (MAC)
pub fn blake3_keyed(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new_keyed(key);
    hasher.update(data);
    hasher.finalize().into()
}

/// BLAKE3 derive key (KDF)
pub fn blake3_derive_key(context: &str, key_material: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new_derive_key(context);
    hasher.update(key_material);
    hasher.finalize().into()
}

// ===================================================================
// BİRLEŞİK HASH FONKSİYONLARI
// ===================================================================

/// İşlem hash'i hesaplar
pub fn hash_transaction(
    from: &[u8],
    to: &[u8],
    amount: u128,
    fee: u128,
    nonce: u64,
    timestamp: u64,
) -> Hash {
    let mut hasher = Sha3_256::new();
    hasher.update(from);
    hasher.update(to);
    hasher.update(&amount.to_le_bytes());
    hasher.update(&fee.to_le_bytes());
    hasher.update(&nonce.to_le_bytes());
    hasher.update(&timestamp.to_le_bytes());
    hasher.finalize().into()
}

/// Blok hash'i hesaplar
pub fn hash_block_header(
    height: u64,
    prev_hash: &Hash,
    tx_root: &Hash,
    state_root: &Hash,
    timestamp: u64,
    proposer: &[u8],
) -> Hash {
    let mut hasher = Sha3_256::new();
    hasher.update(&height.to_le_bytes());
    hasher.update(prev_hash);
    hasher.update(tx_root);
    hasher.update(state_root);
    hasher.update(&timestamp.to_le_bytes());
    hasher.update(proposer);
    hasher.finalize().into()
}

/// Merkle node hash'i hesaplar (çiftli birleştirme)
pub fn hash_merkle_pair(left: &Hash, right: &Hash) -> Hash {
    let mut hasher = Sha3_256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

// ===================================================================
// HASH UTILITIES
// ===================================================================

/// Hash'i hex string'e çevirir
pub fn hash_to_hex(hash: &Hash) -> String {
    format!("0x{}", hex::encode(hash))
}

/// Hex string'den hash parse eder
pub fn hash_from_hex(hex: &str) -> Option<Hash> {
    let hex = hex.trim_start_matches("0x");
    let bytes = hex::decode(hex).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Some(hash)
}

/// İki hash'i karşılaştırır (sabit zamanlı - timing attack önlemi)
pub fn constant_time_compare(a: &Hash, b: &Hash) -> bool {
    use std::cmp::Ordering;
    a.len() == b.len() && 
    a.iter().zip(b.iter()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Hash prefix'ini kontrol eder (PoW için)
pub fn hash_has_prefix(hash: &Hash, prefix: &[u8]) -> bool {
    hash.starts_with(prefix)
}

/// Leading zero bit sayısını hesaplar (difficulty için)
pub fn count_leading_zero_bits(hash: &Hash) -> u32 {
    let mut count = 0;
    for byte in hash {
        if *byte == 0 {
            count += 8;
        } else {
            count += byte.leading_zeros();
            break;
        }
    }
    count
}

// ===================================================================
// HMAC ve KDF
// ===================================================================

/// HMAC-SHA256 hesaplar
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    use sha2::Sha256;
    use hmac::{Hmac, Mac};
    
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .expect("HMAC key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// HKDF (HMAC-based Key Derivation Function)
pub fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    use hkdf::Hkdf;
    use sha2::Sha256;
    
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = vec![0u8; length];
    hk.expand(info, &mut okm)
        .expect("HKDF expand failed");
    okm
}

/// PBKDF2 (Password-Based Key Derivation)
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, length: usize) -> Vec<u8> {
    use pbkdf2::pbkdf2_hmac_array;
    use sha2::Sha256;
    
    pbkdf2_hmac_array::<Sha256, 32>(password, salt, iterations)
        .to_vec()
        .into_iter()
        .take(length)
        .collect()
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        let data = b"Pacyte Nexus";
        let hash = hash_sha256(data);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_sha3_256() {
        let data = b"Pacyte Nexus";
        let hash = hash_sha3_256(data);
        assert_eq!(hash.len(), 32);
        
        // Farklı hash'ler farklı sonuç üretmeli
        let sha256_hash = hash_sha256(data);
        assert_ne!(hash, sha256_hash);
    }

    #[test]
    fn test_blake3() {
        let data = b"Pacyte Nexus";
        let hash = hash_blake3(data);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_hash256() {
        let data = b"Bitcoin style double hash";
        let hash = hash256(data);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_keccak256() {
        let data = b"";
        let hash = hash_keccak256(data);
        let expected = hex::decode("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470").unwrap();
        assert_eq!(hash.as_slice(), expected.as_slice());
    }

    #[test]
    fn test_blake3_keyed() {
        let key = [42u8; 32];
        let data = b"secret data";
        let mac = blake3_keyed(&key, data);
        assert_eq!(mac.len(), 32);
    }

    #[test]
    fn test_blake3_derive_key() {
        let context = "Pacyte Nexus KDF v1";
        let material = b"master secret";
        let derived = blake3_derive_key(context, material);
        assert_eq!(derived.len(), 32);
    }

    #[test]
    fn test_hash_to_hex() {
        let hash = [42u8; 32];
        let hex = hash_to_hex(&hash);
        assert!(hex.starts_with("0x"));
        assert_eq!(hex.len(), 66);
    }

    #[test]
    fn test_hash_from_hex() {
        let hex = "0x2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a";
        let hash = hash_from_hex(hex).unwrap();
        assert_eq!(hash, [42u8; 32]);
        
        assert!(hash_from_hex("invalid").is_none());
        assert!(hash_from_hex("0x123").is_none());
    }

    #[test]
    fn test_constant_time_compare() {
        let a = [1u8; 32];
        let b = [1u8; 32];
        let c = [2u8; 32];
        
        assert!(constant_time_compare(&a, &b));
        assert!(!constant_time_compare(&a, &c));
    }

    #[test]
    fn test_count_leading_zero_bits() {
        let mut hash = [0u8; 32];
        assert_eq!(count_leading_zero_bits(&hash), 256);
        
        hash[0] = 0x0F;
        assert_eq!(count_leading_zero_bits(&hash), 4);
        
        hash[0] = 0xFF;
        assert_eq!(count_leading_zero_bits(&hash), 0);
    }

    #[test]
    fn test_hmac_sha256() {
        let key = b"secret";
        let data = b"message";
        let mac = hmac_sha256(key, data);
        assert_eq!(mac.len(), 32);
    }

    #[test]
    fn test_hkdf() {
        let ikm = b"input key material";
        let salt = b"salt";
        let info = b"Pacyte Nexus";
        let okm = hkdf_sha256(ikm, salt, info, 32);
        assert_eq!(okm.len(), 32);
    }

    #[test]
    fn test_hash_transaction() {
        let from = [1u8; 32];
        let to = [2u8; 32];
        let hash = hash_transaction(&from, &to, 1000, 10, 0, 1234567890);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_hash_block_header() {
        let prev = [0u8; 32];
        let tx_root = [1u8; 32];
        let state_root = [2u8; 32];
        let proposer = [3u8; 32];
        
        let hash = hash_block_header(1, &prev, &tx_root, &state_root, 1234567890, &proposer);
        assert_eq!(hash.len(), 32);
    }
}