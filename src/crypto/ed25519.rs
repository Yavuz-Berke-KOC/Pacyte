// ===================================================================
// PACYTE NEXUS - ED25519 KRİPTOGRAFİ (GERÇEK)
// ===================================================================

use ed25519_dalek::{
    Keypair, PublicKey, SecretKey, Signature, Signer as DalekSigner,
    Verifier as DalekVerifier, SECRET_KEY_LENGTH,
};
use rand::rngs::OsRng;
use sha2::Sha512;
use sha3::{Digest, Sha3_256};

use crate::types::{Address, Hash, PacyteError, PacyteResult, Signature as PacyteSignature};

// ===================================================================
// ED25519 İMZALAYICI
// ===================================================================

// Clone manuel implemente edilecek
pub struct Ed25519Signer {
    keypair: Keypair,
}

impl Clone for Ed25519Signer {
    fn clone(&self) -> Self {
        Self {
            keypair: Keypair::from_bytes(&self.keypair.to_bytes()).unwrap(),
        }
    }
}

impl Ed25519Signer {
    /// Yeni rastgele anahtar çifti oluşturur
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let keypair = Keypair::generate(&mut csprng);
        Self { keypair }
    }
    
    /// Seed'den anahtar çifti oluşturur (deterministik)
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let secret = SecretKey::from_bytes(seed).expect("Invalid seed");
        let public = PublicKey::from(&secret);
        let keypair = Keypair { secret, public };
        Self { keypair }
    }
    
    /// Secret key bytes'ından oluşturur
    pub fn from_secret_bytes(bytes: &[u8]) -> PacyteResult<Self> {
        if bytes.len() != SECRET_KEY_LENGTH {
            return Err(PacyteError::CryptoError(
                format!("Invalid secret key length: {}", bytes.len())
            ));
        }
        
        let secret = SecretKey::from_bytes(bytes)
            .map_err(|e| PacyteError::CryptoError(e.to_string()))?;
        let public = PublicKey::from(&secret);
        let keypair = Keypair { secret, public };
        
        Ok(Self { keypair })
    }
    
    /// Mesajı imzalar
    pub fn sign(&self, message: &[u8]) -> PacyteSignature {
        let signature: Signature = self.keypair.sign(message);
        signature.to_bytes().to_vec()
    }
    
    /// Public key bytes'ını döndürür
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.keypair.public.as_bytes().to_vec()
    }
    
    /// Secret key bytes'ını döndürür (DİKKAT: Güvenli saklayın!)
    pub fn secret_key_bytes(&self) -> Vec<u8> {
        self.keypair.secret.to_bytes().to_vec()
    }
    
    /// Adresi döndürür (public key'in SHA3-256 hash'i)
    pub fn address(&self) -> Address {
        let mut hasher = Sha3_256::new();
        hasher.update(self.keypair.public.as_bytes());
        hasher.finalize().into()
    }
    
    /// Detached imza oluşturur
    pub fn sign_detached(&self, message: &[u8]) -> PacyteSignature {
        self.sign(message)
    }
}

impl std::fmt::Debug for Ed25519Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ed25519Signer")
            .field("public_key", &hex::encode(&self.public_key_bytes()[..8]))
            .finish()
    }
}

impl crate::crypto::Signer for Ed25519Signer {
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
// ED25519 DOĞRULAYICI
// ===================================================================

pub struct Ed25519Verifier;

impl Ed25519Verifier {
    /// İmzayı doğrular
    pub fn verify(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        // Public key parse
        let pk = match PublicKey::from_bytes(public_key) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        
        // Signature parse
        let sig = match Signature::from_bytes(signature) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        
        // Doğrula
        pk.verify(message, &sig).is_ok()
    }
    
    /// Public key'den adres türetir
    pub fn address_from_pk(public_key: &[u8]) -> Address {
        let mut hasher = Sha3_256::new();
        hasher.update(public_key);
        hasher.finalize().into()
    }
    
    /// Batch doğrulama (çoklu imza)
    pub fn verify_batch(
        messages: &[&[u8]],
        signatures: &[&[u8]],
        public_keys: &[&[u8]],
    ) -> bool {
        if messages.len() != signatures.len() || messages.len() != public_keys.len() {
            return false;
        }
        
        // Her bir imzayı ayrı ayrı doğrula
        // (ed25519-dalek batch verification için özel API var ama basit tutalım)
        for i in 0..messages.len() {
            if !Self::verify(messages[i], signatures[i], public_keys[i]) {
                return false;
            }
        }
        true
    }
}

impl crate::crypto::Verifier for Ed25519Verifier {
    fn verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        Self::verify(message, signature, public_key)
    }
    
    fn address_from_pk(public_key: &[u8]) -> Address {
        Self::address_from_pk(public_key)
    }
}

// ===================================================================
// ED25519 ÇOKLU İMZA (M-of-N)
// ===================================================================

pub struct MultiSigEd25519 {
    threshold: usize,
    public_keys: Vec<PublicKey>,
}

impl MultiSigEd25519 {
    pub fn new(threshold: usize, public_keys: Vec<Vec<u8>>) -> PacyteResult<Self> {
        if threshold == 0 || threshold > public_keys.len() {
            return Err(PacyteError::CryptoError(
                format!("Invalid threshold: {}/{}", threshold, public_keys.len())
            ));
        }
        
        let pks: Result<Vec<PublicKey>, _> = public_keys
            .iter()
            .map(|pk| PublicKey::from_bytes(pk))
            .collect();
        
        let pks = pks.map_err(|e| PacyteError::CryptoError(e.to_string()))?;
        
        Ok(Self {
            threshold,
            public_keys: pks,
        })
    }
    
    pub fn verify(&self, message: &[u8], signatures: &[Vec<u8>]) -> bool {
        if signatures.len() < self.threshold {
            return false;
        }
        
        let mut valid_count = 0;
        let mut used_keys = std::collections::HashSet::new();
        
        for sig_bytes in signatures {
            let sig = match Signature::from_bytes(sig_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            
            for (idx, pk) in self.public_keys.iter().enumerate() {
                if used_keys.contains(&idx) {
                    continue;
                }
                
                if pk.verify(message, &sig).is_ok() {
                    valid_count += 1;
                    used_keys.insert(idx);
                    break;
                }
            }
            
            if valid_count >= self.threshold {
                return true;
            }
        }
        
        false
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
        let signer = Ed25519Signer::generate();
        let msg = b"Hello, Pacyte Nexus!";
        
        let sig = signer.sign(msg);
        let pk = signer.public_key_bytes();
        
        assert!(Ed25519Verifier::verify(msg, &sig, &pk));
    }

    #[test]
    fn test_invalid_signature() {
        let signer1 = Ed25519Signer::generate();
        let signer2 = Ed25519Signer::generate();
        let msg = b"Test message";
        
        let sig = signer1.sign(msg);
        let wrong_pk = signer2.public_key_bytes();
        
        assert!(!Ed25519Verifier::verify(msg, &sig, &wrong_pk));
    }

    #[test]
    fn test_tampered_message() {
        let signer = Ed25519Signer::generate();
        let msg = b"Original message";
        let tampered = b"Tampered message";
        
        let sig = signer.sign(msg);
        let pk = signer.public_key_bytes();
        
        assert!(!Ed25519Verifier::verify(tampered, &sig, &pk));
    }

    #[test]
    fn test_deterministic_from_seed() {
        let seed = [42u8; 32];
        let signer1 = Ed25519Signer::from_seed(&seed);
        let signer2 = Ed25519Signer::from_seed(&seed);
        
        assert_eq!(signer1.public_key_bytes(), signer2.public_key_bytes());
        assert_eq!(signer1.secret_key_bytes(), signer2.secret_key_bytes());
    }

    #[test]
    fn test_address_generation() {
        let signer = Ed25519Signer::generate();
        let addr = signer.address();
        
        assert_eq!(addr.len(), 32);
        
        let addr2 = Ed25519Verifier::address_from_pk(&signer.public_key_bytes());
        assert_eq!(addr, addr2);
    }

    #[test]
    fn test_batch_verification() {
        let signers: Vec<_> = (0..5).map(|_| Ed25519Signer::generate()).collect();
        let messages: Vec<_> = (0..5).map(|i| format!("Message {}", i).into_bytes()).collect();
        
        let signatures: Vec<_> = signers.iter()
            .enumerate()
            .map(|(i, s)| s.sign(&messages[i]))
            .collect();
        
        let pks: Vec<_> = signers.iter()
            .map(|s| s.public_key_bytes())
            .collect();
        
        let msg_refs: Vec<_> = messages.iter().map(|m| m.as_slice()).collect();
        let sig_refs: Vec<_> = signatures.iter().map(|s| s.as_slice()).collect();
        let pk_refs: Vec<_> = pks.iter().map(|pk| pk.as_slice()).collect();
        
        assert!(Ed25519Verifier::verify_batch(&msg_refs, &sig_refs, &pk_refs));
    }

    #[test]
    fn test_multisig() {
        let signers: Vec<_> = (0..5).map(|_| Ed25519Signer::generate()).collect();
        let msg = b"Multi-sig transaction";
        
        let pks: Vec<_> = signers.iter().map(|s| s.public_key_bytes()).collect();
        let multisig = MultiSigEd25519::new(3, pks).unwrap();
        
        // 2 imza yetmez
        let sigs: Vec<_> = signers[0..2].iter().map(|s| s.sign(msg)).collect();
        assert!(!multisig.verify(msg, &sigs));
        
        // 3 imza yeter
        let sigs: Vec<_> = signers[0..3].iter().map(|s| s.sign(msg)).collect();
        assert!(multisig.verify(msg, &sigs));
    }

    #[test]
    fn test_signature_serialization() {
        let signer = Ed25519Signer::generate();
        let msg = b"Test";
        
        let sig = signer.sign(msg);
        assert_eq!(sig.len(), 64); // Ed25519 imza boyutu
        
        // Round-trip test
        let parsed = Signature::from_bytes(&sig).unwrap();
        assert_eq!(parsed.to_bytes().to_vec(), sig);
    }
}