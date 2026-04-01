use std::{collections::HashSet, sync::Arc};

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use api_types::User;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{auth::provider::ProviderTokenDetails, db::auth::AuthSession};

pub(super) const ACCESS_TOKEN_TTL_SECONDS: i64 = 120;
pub(super) const REFRESH_TOKEN_TTL_DAYS: i64 = 365;
const DEFAULT_JWT_LEEWAY_SECONDS: u64 = 60;

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("invalid token")]
    InvalidToken,
    #[error("invalid jwt secret")]
    InvalidSecret,
    #[error("token expired")]
    TokenExpired,
    #[error("refresh token reused - possible theft detected")]
    TokenReuseDetected,
    #[error("session revoked")]
    SessionRevoked,
    #[error("token type mismatch")]
    InvalidTokenType,
    #[error("encryption error")]
    EncryptionError,
    #[error("serialization error")]
    SerializationError,
    #[error(transparent)]
    Jwt(#[from] jsonwebtoken::errors::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct AccessTokenClaims {
    pub sub: Uuid,
    pub session_id: Uuid,
    pub iat: i64,
    pub exp: i64,
    pub aud: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RefreshTokenClaims {
    pub sub: Uuid,
    pub session_id: Uuid,
    pub jti: Uuid,
    pub iat: i64,
    pub exp: i64,
    pub aud: String,
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_tokens_blob: Option<String>, // Legacy claim for older refresh tokens
}

#[derive(Debug, Clone)]
pub struct AccessTokenDetails {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RefreshTokenDetails {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub refresh_token_id: Uuid,
    pub provider: String,
    pub legacy_provider_token_details: Option<ProviderTokenDetails>,
}

#[derive(Clone)]
pub struct JwtService {
    pub secret: Arc<SecretString>,
}

#[derive(Debug, Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub refresh_token_id: Uuid,
}

impl JwtService {
    pub fn new(secret: SecretString) -> Self {
        Self {
            secret: Arc::new(secret),
        }
    }

    pub fn generate_tokens(
        &self,
        session: &AuthSession,
        user: &User,
        provider: &str,
    ) -> Result<Tokens, JwtError> {
        let now = Utc::now();
        let refresh_token_id = Uuid::new_v4();

        self.generate_tokens_for_refresh_token_id(session, user.id, provider, refresh_token_id, now)
    }

    pub fn generate_tokens_for_refresh_token_id(
        &self,
        session: &AuthSession,
        user_id: Uuid,
        provider: &str,
        refresh_token_id: Uuid,
        issued_at: DateTime<Utc>,
    ) -> Result<Tokens, JwtError> {
        let now = Utc::now();

        // Access token, short-lived (~2 minutes)
        let access_exp = now + ChronoDuration::seconds(ACCESS_TOKEN_TTL_SECONDS);
        let access_claims = AccessTokenClaims {
            sub: user_id,
            session_id: session.id,
            iat: now.timestamp(),
            exp: access_exp.timestamp(),
            aud: "access".to_string(),
        };

        // Refresh token, long-lived (~1 year)
        let refresh_exp = issued_at + ChronoDuration::days(REFRESH_TOKEN_TTL_DAYS);
        let refresh_claims = RefreshTokenClaims {
            sub: user_id,
            session_id: session.id,
            jti: refresh_token_id,
            iat: issued_at.timestamp(),
            exp: refresh_exp.timestamp(),
            aud: "refresh".to_string(),
            provider: Some(provider.to_string()),
            provider_tokens_blob: None,
        };

        let encoding_key = EncodingKey::from_base64_secret(self.secret.expose_secret())?;

        let access_token = encode(
            &Header::new(Algorithm::HS256),
            &access_claims,
            &encoding_key,
        )?;

        let refresh_token = encode(
            &Header::new(Algorithm::HS256),
            &refresh_claims,
            &encoding_key,
        )?;

        Ok(Tokens {
            access_token,
            refresh_token,
            refresh_token_id,
        })
    }

    pub fn decode_access_token(&self, token: &str) -> Result<AccessTokenDetails, JwtError> {
        self.decode_access_token_with_leeway(token, DEFAULT_JWT_LEEWAY_SECONDS)
    }

    pub fn decode_access_token_with_leeway(
        &self,
        token: &str,
        leeway_seconds: u64,
    ) -> Result<AccessTokenDetails, JwtError> {
        if token.trim().is_empty() {
            return Err(JwtError::InvalidToken);
        }

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        validation.validate_nbf = false;
        validation.set_audience(&["access"]);
        validation.required_spec_claims =
            HashSet::from(["sub".to_string(), "exp".to_string(), "aud".to_string()]);
        validation.leeway = leeway_seconds;

        let decoding_key = DecodingKey::from_base64_secret(self.secret.expose_secret())?;
        let data = decode::<AccessTokenClaims>(token, &decoding_key, &validation)?;
        let claims = data.claims;
        let expires_at = DateTime::from_timestamp(claims.exp, 0).ok_or(JwtError::InvalidToken)?;

        Ok(AccessTokenDetails {
            user_id: claims.sub,
            session_id: claims.session_id,
            expires_at,
        })
    }

    pub fn decode_refresh_token(&self, token: &str) -> Result<RefreshTokenDetails, JwtError> {
        if token.trim().is_empty() {
            return Err(JwtError::InvalidToken);
        }

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        validation.validate_nbf = false;
        validation.set_audience(&["refresh"]);
        validation.required_spec_claims = HashSet::from([
            "sub".to_string(),
            "exp".to_string(),
            "aud".to_string(),
            "jti".to_string(),
        ]);
        validation.leeway = DEFAULT_JWT_LEEWAY_SECONDS;

        let decoding_key = DecodingKey::from_base64_secret(self.secret.expose_secret())?;
        let data = decode::<RefreshTokenClaims>(token, &decoding_key, &validation)?;
        let claims = data.claims;

        let (provider, legacy_provider_token_details) =
            if let Some(provider) = claims.provider.as_ref().filter(|p| !p.trim().is_empty()) {
                (provider.to_string(), None)
            } else if let Some(provider_tokens_blob) = claims.provider_tokens_blob.as_deref() {
                let provider_token_details = self.decrypt_provider_tokens(provider_tokens_blob)?;
                (
                    provider_token_details.provider.clone(),
                    Some(provider_token_details),
                )
            } else {
                return Err(JwtError::InvalidToken);
            };

        Ok(RefreshTokenDetails {
            user_id: claims.sub,
            session_id: claims.session_id,
            refresh_token_id: claims.jti,
            provider,
            legacy_provider_token_details,
        })
    }

    pub fn decrypt_provider_tokens(
        &self,
        provider_tokens_blob: &str,
    ) -> Result<ProviderTokenDetails, JwtError> {
        let decrypted = self.decrypt_data(provider_tokens_blob)?;
        let decrypted_str = String::from_utf8_lossy(&decrypted);
        serde_json::from_str(&decrypted_str).map_err(|_| JwtError::InvalidToken)
    }

    pub fn encrypt_provider_tokens(
        &self,
        provider_tokens: &ProviderTokenDetails,
    ) -> Result<String, JwtError> {
        let json =
            serde_json::to_string(provider_tokens).map_err(|_| JwtError::SerializationError)?;
        self.encrypt_data(json.as_bytes())
    }

    /// Encrypt arbitrary bytes and return a base64-encoded blob.
    /// Used by Jira credential storage and other integrations.
    pub fn encrypt_provider_tokens_raw(&self, data: &[u8]) -> Result<Vec<u8>, JwtError> {
        let encoded = self.encrypt_data(data)?;
        Ok(encoded.into_bytes())
    }

    /// Decrypt a blob previously encrypted with `encrypt_provider_tokens_raw`.
    /// Returns the decrypted data as a UTF-8 string.
    pub fn decrypt_provider_tokens_raw(&self, encrypted: &[u8]) -> Result<String, JwtError> {
        let encoded_str = std::str::from_utf8(encrypted).map_err(|_| JwtError::InvalidToken)?;
        let decrypted = self.decrypt_data(encoded_str)?;
        String::from_utf8(decrypted).map_err(|_| JwtError::InvalidToken)
    }

    fn encrypt_data(&self, data: &[u8]) -> Result<String, JwtError> {
        let key_bytes = self.derive_key()?;
        let key = Key::<Aes256Gcm>::from(key_bytes);
        let cipher = Aes256Gcm::new(&key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, data)
            .map_err(|_| JwtError::EncryptionError)?;

        let mut combined = nonce.to_vec();
        combined.extend_from_slice(&ciphertext);

        Ok(URL_SAFE_NO_PAD.encode(combined))
    }

    fn decrypt_data(&self, encrypted: &str) -> Result<Vec<u8>, JwtError> {
        let decoded = URL_SAFE_NO_PAD
            .decode(encrypted)
            .map_err(|_| JwtError::InvalidToken)?;

        const NONCE_SIZE: usize = 12; // 96 bits for AES-256-GCM
        if decoded.len() < NONCE_SIZE {
            return Err(JwtError::InvalidToken);
        }

        let key_bytes = self.derive_key()?;
        let key = Key::<Aes256Gcm>::from(key_bytes);
        let cipher = Aes256Gcm::new(&key);
        let nonce_bytes: [u8; NONCE_SIZE] = decoded[..NONCE_SIZE]
            .try_into()
            .map_err(|_| JwtError::InvalidToken)?;
        let nonce = Nonce::from(nonce_bytes);
        let ciphertext = &decoded[NONCE_SIZE..];

        cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| JwtError::EncryptionError)
    }

    fn derive_key(&self) -> Result<[u8; 32], JwtError> {
        let secret_bytes = STANDARD
            .decode(self.secret.expose_secret())
            .map_err(|_| JwtError::InvalidSecret)?;

        let mut hasher = Sha256::new();
        hasher.update(&secret_bytes);
        Ok(hasher.finalize().into())
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;

    /// Build a JwtService with a fixed base64-encoded 32-byte secret for tests.
    /// `derive_key()` base64-decodes the secret, so we provide a valid base64 string.
    fn test_jwt_service() -> JwtService {
        // base64(32 zero bytes) = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        JwtService::new(SecretString::from(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
        ))
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let svc = test_jwt_service();
        let plaintext = b"jira-oauth2-credentials-json-payload";
        let encrypted = svc
            .encrypt_provider_tokens_raw(plaintext)
            .expect("encryption should succeed");
        let decrypted = svc
            .decrypt_provider_tokens_raw(&encrypted)
            .expect("decryption should succeed");
        assert_eq!(decrypted.as_bytes(), plaintext);
    }

    #[test]
    fn test_decrypt_garbage_returns_error() {
        let svc = test_jwt_service();
        let result = svc.decrypt_provider_tokens_raw(b"not-valid-ciphertext");
        assert!(result.is_err(), "garbage input should return an error");
    }

    #[test]
    fn test_nonce_is_random_per_encryption() {
        // Encrypting the same plaintext twice must produce different ciphertexts
        // (because AES-256-GCM uses a random nonce each time).
        let svc = test_jwt_service();
        let data = b"same data every time";
        let enc1 = svc.encrypt_provider_tokens_raw(data).unwrap();
        let enc2 = svc.encrypt_provider_tokens_raw(data).unwrap();
        assert_ne!(
            enc1, enc2,
            "two encryptions of the same data should differ (random nonce)"
        );
    }

    #[test]
    fn test_wrong_key_cannot_decrypt() {
        let svc1 = test_jwt_service();
        // A different base64 secret = different derived key
        let svc2 = JwtService::new(SecretString::from(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB=".to_string(),
        ));
        let encrypted = svc1.encrypt_provider_tokens_raw(b"secret").unwrap();
        let result = svc2.decrypt_provider_tokens_raw(&encrypted);
        assert!(
            result.is_err(),
            "data encrypted with key1 should not decrypt with key2"
        );
    }
}
