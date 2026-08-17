// ===================================================================
// PACYTE NEXUS - KRİPTOGRAFİ MODÜLÜ
// ===================================================================

pub mod ed25519;
pub mod dilithium5;
pub mod hash;
pub mod merkle;

// Re-export'lar
pub use ed25519::*;
pub use dilithium5::*;
pub use hash::*;
pub use merkle::*;

use crate::types::{PacyteError, PacyteResult, Address, Hash, Signature, PublicKeyBytes};

// ===================================================================
// KRİPTOGRAFİ TRAIT'LERİ
// ===================================================================

/// Genel imza trait'i
pub trait Signer: Send + Sync {
    /// Mesajı imzala
    fn sign(&self, message: &[u8]) -> Signature;
    
    /// Public key'i döndür
    fn public_key_bytes(&self) -> PublicKeyBytes;
    
    /// Adresi döndür (public key'in hash'i)
    fn address(&self) -> Address;
}

/// Genel doğrulama trait'i
pub trait Verifier: Send + Sync {
    /// İmzayı doğrula
    fn verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> bool;
    
    /// Public key'den adres türet
    fn address_from_pk(public_key: &[u8]) -> Address;
}

// ===================================================================
// ANAHTAR TİPLERİ
// ===================================================================

#[derive(Debug, Clone)]
pub enum KeyType {
    Ed25519,
    Dilithium5,
}

impl KeyType {
    pub fn from_signature_len(len: usize) -> Option<Self> {
        match len {
            64 => Some(KeyType::Ed25519),
            4595 => Some(KeyType::Dilithium5),
            _ => None,
        }
    }
    
    pub fn from_public_key_len(len: usize) -> Option<Self> {
        match len {
            32 => Some(KeyType::Ed25519),
            2592 => Some(KeyType::Dilithium5),
            _ => None,
        }
    }
}

// ===================================================================
// BİRLEŞİK İMZALAYICI (Ed25519 + Dilithium5)
// ===================================================================

#[derive(Debug, Clone)]
pub enum HybridSigner {
    Ed25519(Ed25519Signer),
    Dilithium5(Dilithium5Signer),
    Both {
        ed25519: Ed25519Signer,
        dilithium5: Dilithium5Signer,
    },
}

impl HybridSigner {
    pub fn new_ed25519() -> Self {
        Self::Ed25519(Ed25519Signer::generate())
    }
    
    pub fn new_dilithium5() -> Self {
        Self::Dilithium5(Dilithium5Signer::generate())
    }
    
    pub fn new_both() -> Self {
        Self::Both {
            ed25519: Ed25519Signer::generate(),
            dilithium5: Dilithium5Signer::generate(),
        }
    }
    
    pub fn sign(&self, message: &[u8]) -> HybridSignature {
        match self {
            Self::Ed25519(s) => HybridSignature::Ed25519(s.sign(message)),
            Self::Dilithium5(s) => HybridSignature::Dilithium5(s.sign(message)),
            Self::Both { ed25519, dilithium5 } => HybridSignature::Both {
                ed25519: ed25519.sign(message),
                dilithium5: dilithium5.sign(message),
            },
        }
    }
    
    pub fn public_keys(&self) -> HybridPublicKey {
        match self {
            Self::Ed25519(s) => HybridPublicKey::Ed25519(s.public_key_bytes()),
            Self::Dilithium5(s) => HybridPublicKey::Dilithium5(s.public_key_bytes()),
            Self::Both { ed25519, dilithium5 } => HybridPublicKey::Both {
                ed25519: ed25519.public_key_bytes(),
                dilithium5: dilithium5.public_key_bytes(),
            },
        }
    }
    
    pub fn address(&self) -> Address {
        match self {
            Self::Ed25519(s) => s.address(),
            Self::Dilithium5(s) => s.address(),
            Self::Both { ed25519, .. } => ed25519.address(),
        }
    }
}

// ===================================================================
// HİBRİT İMZA
// ===================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HybridSignature {
    Ed25519(Vec<u8>),
    Dilithium5(Vec<u8>),
    Both {
        ed25519: Vec<u8>,
        dilithium5: Vec<u8>,
    },
}

impl HybridSignature {
    pub fn verify(&self, message: &[u8], public_key: &HybridPublicKey) -> bool {
        match (self, public_key) {
            (Self::Ed25519(sig), HybridPublicKey::Ed25519(pk)) => {
                Ed25519Verifier::verify(message, sig, pk)
            }
            (Self::Dilithium5(sig), HybridPublicKey::Dilithium5(pk)) => {
                Dilithium5Verifier::verify(message, sig, pk)
            }
            (Self::Both { ed25519, dilithium5 }, HybridPublicKey::Both { ed25519: pk1, dilithium5: pk2 }) => {
                Ed25519Verifier::verify(message, ed25519, pk1) &&
                Dilithium5Verifier::verify(message, dilithium5, pk2)
            }
            _ => false,
        }
    }
    
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
    
    pub fn from_bytes(bytes: &[u8]) -> PacyteResult<Self> {
        bincode::deserialize(bytes).map_err(|e| PacyteError::CryptoError(e.to_string()))
    }
}

// ===================================================================
// HİBRİT PUBLIC KEY
// ===================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HybridPublicKey {
    Ed25519(Vec<u8>),
    Dilithium5(Vec<u8>),
    Both {
        ed25519: Vec<u8>,
        dilithium5: Vec<u8>,
    },
}

impl HybridPublicKey {
    pub fn key_type(&self) -> KeyType {
        match self {
            Self::Ed25519(_) => KeyType::Ed25519,
            Self::Dilithium5(_) => KeyType::Dilithium5,
            Self::Both { .. } => KeyType::Dilithium5,
        }
    }
    
    pub fn to_address(&self) -> Address {
        let bytes = self.to_bytes();
        hash_sha3_256(&bytes)
    }
    
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_ed25519() {
        let signer = HybridSigner::new_ed25519();
        let msg = b"Pacyte Nexus Test";
        
        let sig = signer.sign(msg);
        let pk = signer.public_keys();
        
        assert!(sig.verify(msg, &pk));
    }

    #[test]
    fn test_hybrid_dilithium5() {
        let signer = HybridSigner::new_dilithium5();
        let msg = b"Pacyte Nexus Test - Post Quantum";
        
        let sig = signer.sign(msg);
        let pk = signer.public_keys();
        
        assert!(sig.verify(msg, &pk));
    }

    #[test]
    fn test_hybrid_both() {
        let signer = HybridSigner::new_both();
        let msg = b"Pacyte Nexus Test - Hybrid";
        
        let sig = signer.sign(msg);
        let pk = signer.public_keys();
        
        assert!(sig.verify(msg, &pk));
    }

    #[test]
    fn test_key_type_detection() {
        assert!(matches!(
            KeyType::from_signature_len(64),
            Some(KeyType::Ed25519)
        ));
        assert!(matches!(
            KeyType::from_signature_len(4595),
            Some(KeyType::Dilithium5)
        ));
        assert!(KeyType::from_signature_len(100).is_none());
    }

    #[test]
    fn test_address_generation() {
        let signer = HybridSigner::new_ed25519();
        let addr = signer.address();
        assert_eq!(addr.len(), 32);
        assert_ne!(addr, [0u8; 32]);
    }
}