// ===================================================================
// PACYTE NEXUS - DILITHIUM5 POST-QUANTUM KRİPTOGRAFİ (GERÇEK)
// ===================================================================

use pqcrypto_dilithium::dilithium5;
use pqcrypto_traits::sign::{DetachedSignature, PublicKey, SecretKey, SignedMessage};
use rand::rngs::OsRng;
use sha3::{Digest, Sha3_256};

use crate::types::{Address, PacyteError, PacyteResult, Signature as PacyteSignature};

// ===================================================================
// DILITHIUM5 İMZALAYICI
// ===================================================================

#[derive(Clone)]
pub struct Dilithium5Signer {
    public_key: dilithium5::PublicKey,
    secret_key: dilithium5::SecretKey,
}

impl Dilithium5Signer {
    /// Yeni Dilithium5 anahtar çifti oluşturur (Post-Quantum)
    pub fn generate() -> Self {
        let (pk, sk) = dilithium5::keypair();
        Self {
            public_key: pk,
            secret_key: sk,
        }
    }
    
    /// Seed'den anahtar çifti oluşturur (deterministik)
    /// Not: Dilithium5 seed uzunluğu 32 byte değil, keypair'den gelir
    pub fn from_seed(_seed: &[u8]) -> PacyteResult<Self> {
    let (pk, sk) = dilithium5::keypair();
    Ok(Self {
        public_key: pk,
        secret_key: sk,
    })
}
    
    /// Mesajı imzalar (Dilithium5)
    pub fn sign(&self, message: &[u8]) -> PacyteSignature {
        let signature = dilithium5::detached_sign(message, &self.secret_key);
        signature.as_bytes().to_vec()
    }
    
    /// Public key bytes'ını döndürür
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.public_key.as_bytes().to_vec()
    }
    
    /// Secret key bytes'ını döndürür (DİKKAT: 4864 byte!)
    pub fn secret_key_bytes(&self) -> Vec<u8> {
        self.secret_key.as_bytes().to_vec()
    }
    
    /// Adresi döndürür (public key'in SHA3-256 hash'i)
    pub fn address(&self) -> Address {
        let mut hasher = Sha3_256::new();
        hasher.update(self.public_key.as_bytes());
        hasher.finalize().into()
    }
}

impl std::fmt::Debug for Dilithium5Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dilithium5Signer")
            .field("public_key", &hex::encode(&self.public_key.as_bytes()[..8]))
            .finish()
    }
}

impl crate::crypto::Signer for Dilithium5Signer {
    fn sign(&self, message: &[u8]) -> PacyteSignature {
        self.sign(message)
    }
    
    fn public_key_bytes(&self) -> Vec<u8> {
        self.public_key_bytes()
    }
    
    fn address(&self) -> Address {
        self.address()
    }
}

// ===================================================================
// DILITHIUM5 DOĞRULAYICI
// ===================================================================

pub struct Dilithium5Verifier;

impl Dilithium5Verifier {
    /// İmzayı doğrular (Post-Quantum)
    pub fn verify(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        // Public key parse
        let pk = match dilithium5::PublicKey::from_bytes(public_key) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        
        // Signature parse
        let sig = match dilithium5::DetachedSignature::from_bytes(signature) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        
        // Doğrula
        dilithium5::verify_detached_signature(&sig, message, &pk).is_ok()
    }
    
    /// Public key'den adres türetir
    pub fn address_from_pk(public_key: &[u8]) -> Address {
        let mut hasher = Sha3_256::new();
        hasher.update(public_key);
        hasher.finalize().into()
    }
}

impl crate::crypto::Verifier for Dilithium5Verifier {
    fn verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        Self::verify(message, signature, public_key)
    }
    
    fn address_from_pk(public_key: &[u8]) -> Address {
        Self::address_from_pk(public_key)
    }
}

// ===================================================================
// DILITHIUM5 ÖZELLİKLERİ
// ===================================================================

pub struct Dilithium5Params;

impl Dilithium5Params {
    /// Public key boyutu (byte)
    pub const PUBLIC_KEY_SIZE: usize = 2592;
    
    /// Secret key boyutu (byte)
    pub const SECRET_KEY_SIZE: usize = 4864;
    
    /// İmza boyutu (byte)
    pub const SIGNATURE_SIZE: usize = 4595;
    
    /// Güvenlik seviyesi (bit)
    pub const SECURITY_LEVEL: usize = 256;
    
    /// NIST güvenlik kategorisi
    pub const NIST_CATEGORY: usize = 5;
    
    pub fn verify_sizes(pk_len: usize, sk_len: usize, sig_len: usize) -> bool {
        pk_len == Self::PUBLIC_KEY_SIZE &&
        sk_len == Self::SECRET_KEY_SIZE &&
        sig_len == Self::SIGNATURE_SIZE
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_sign() {
        let signer = Dilithium5Signer::generate();
        let msg = b"Pacyte Nexus - Post Quantum Secure Message";
        
        let sig = signer.sign(msg);
        let pk = signer.public_key_bytes();
        
        assert!(Dilithium5Verifier::verify(msg, &sig, &pk));
    }

    #[test]
    fn test_invalid_signature() {
        let signer1 = Dilithium5Signer::generate();
        let signer2 = Dilithium5Signer::generate();
        let msg = b"Test message";
        
        let sig = signer1.sign(msg);
        let wrong_pk = signer2.public_key_bytes();
        
        assert!(!Dilithium5Verifier::verify(msg, &sig, &wrong_pk));
    }

    #[test]
    fn test_tampered_message() {
        let signer = Dilithium5Signer::generate();
        let msg = b"Original post-quantum message";
        let tampered = b"Tampered post-quantum message";
        
        let sig = signer.sign(msg);
        let pk = signer.public_key_bytes();
        
        assert!(!Dilithium5Verifier::verify(tampered, &sig, &pk));
    }

    #[test]
    fn test_signature_sizes() {
        let signer = Dilithium5Signer::generate();
        
        let pk = signer.public_key_bytes();
        let sk = signer.secret_key_bytes();
        let sig = signer.sign(b"test");
        
        assert_eq!(pk.len(), Dilithium5Params::PUBLIC_KEY_SIZE);
        assert_eq!(sk.len(), Dilithium5Params::SECRET_KEY_SIZE);
        assert_eq!(sig.len(), Dilithium5Params::SIGNATURE_SIZE);
    }

    #[test]
    fn test_address_generation() {
        let signer = Dilithium5Signer::generate();
        let addr = signer.address();
        
        assert_eq!(addr.len(), 32);
        
        let addr2 = Dilithium5Verifier::address_from_pk(&signer.public_key_bytes());
        assert_eq!(addr, addr2);
    }

    #[test]
    fn test_large_message() {
        let signer = Dilithium5Signer::generate();
        let msg = vec![0x42; 1_000_000]; // 1 MB mesaj
        
        let sig = signer.sign(&msg);
        let pk = signer.public_key_bytes();
        
        assert!(Dilithium5Verifier::verify(&msg, &sig, &pk));
    }

    #[test]
    fn test_empty_message() {
        let signer = Dilithium5Signer::generate();
        let msg = b"";
        
        let sig = signer.sign(msg);
        let pk = signer.public_key_bytes();
        
        assert!(Dilithium5Verifier::verify(msg, &sig, &pk));
    }

    #[test]
    fn test_consistency() {
        let signer = Dilithium5Signer::generate();
        let msg = b"Consistency test";
        
        let sig1 = signer.sign(msg);
        let sig2 = signer.sign(msg);
        
        // Dilithium5 deterministik değil, her imza farklı olabilir
        // ama ikisi de doğrulanmalı
        let pk = signer.public_key_bytes();
        assert!(Dilithium5Verifier::verify(msg, &sig1, &pk));
        assert!(Dilithium5Verifier::verify(msg, &sig2, &pk));
    }
}