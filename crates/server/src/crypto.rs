#[allow(deprecated)] // aes-gcm 0.10 uses deprecated generic-array APIs internally
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;

const LOCAL_ENCRYPTION_SALT: &[u8] = b"vibe-kanban-local-jira-v2";

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Encryption failed")]
    EncryptionFailed,
    #[error("Decryption failed")]
    DecryptionFailed,
    #[error("Invalid encrypted data: too short")]
    InvalidData,
}

pub struct LocalCrypto {
    cipher: Aes256Gcm,
}

#[allow(deprecated)] // aes-gcm 0.10 uses deprecated generic-array 0.x APIs
impl LocalCrypto {
    pub fn new(user_id: &str) -> Self {
        // NOTE: Key is derived from user_id alone. For the local single-user
        // app the user_id is a fixed UUID, so the key is effectively static
        // per installation. This is acceptable because the SQLite DB file
        // itself is the trust boundary — anyone who can read the DB can also
        // read the binary and extract the salt. A future improvement could
        // store a random machine secret in the OS keychain.
        let hk = Hkdf::<Sha256>::new(Some(LOCAL_ENCRYPTION_SALT), user_id.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"vibe-kanban-aes-key", &mut key)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        Self {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key)),
        }
    }

    pub fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, data)
            .map_err(|_| CryptoError::EncryptionFailed)?;
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(nonce.as_slice());
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    pub fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < 12 {
            return Err(CryptoError::InvalidData);
        }
        let (nonce_bytes, ciphertext) = blob.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| CryptoError::DecryptionFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let crypto = LocalCrypto::new("test-user-id");
        let plaintext = b"secret jira credentials json";
        let encrypted = crypto.encrypt(plaintext).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_keys_cannot_decrypt() {
        let crypto1 = LocalCrypto::new("user-1");
        let crypto2 = LocalCrypto::new("user-2");
        let encrypted = crypto1.encrypt(b"secret").unwrap();
        assert!(crypto2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_invalid_data_too_short() {
        let crypto = LocalCrypto::new("user");
        assert!(crypto.decrypt(&[0u8; 5]).is_err());
    }
}
