//! 对话加密/解密

use crate::error::GaggleError;
use crate::negotiation::space::EncryptedContent;
use aes_gcm::aead::{generic_array::GenericArray, Aead};
use aes_gcm::{Aes256Gcm, NewAead, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::Rng;
use sha2::Digest;

/// 加密内容
pub fn encrypt_content(plaintext: &str, key: &str) -> Result<EncryptedContent, GaggleError> {
    let key_bytes = derive_key(key);
    let key_array = GenericArray::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key_array);

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| GaggleError::EncryptionError(format!("encrypt: {:?}", e)))?;

    Ok(EncryptedContent::new(
        BASE64.encode(&ciphertext),
        BASE64.encode(nonce_bytes),
    ))
}

/// 解密内容
pub fn decrypt_content(encrypted: &EncryptedContent, key: &str) -> Result<String, GaggleError> {
    let key_bytes = derive_key(key);
    let key_array = GenericArray::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key_array);

    let nonce_bytes = BASE64
        .decode(&encrypted.nonce)
        .map_err(|e| GaggleError::EncryptionError(format!("decode nonce: {:?}", e)))?;

    if nonce_bytes.len() != 12 {
        return Err(GaggleError::EncryptionError(
            "Invalid nonce length".to_string(),
        ));
    }

    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = BASE64
        .decode(&encrypted.cipher)
        .map_err(|e| GaggleError::EncryptionError(format!("decode cipher: {:?}", e)))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| GaggleError::EncryptionError(format!("decrypt: {:?}", e)))?;

    String::from_utf8(plaintext).map_err(|e| GaggleError::EncryptionError(format!("utf8: {:?}", e)))
}

/// 从字符串派生256位密钥
fn derive_key(key: &str) -> [u8; 32] {
    let mut hasher = sha2::Sha256::new();
    hasher.update(key.as_bytes());
    let result = hasher.finalize();
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&result);
    key_bytes
}

/// 生成随机对称密钥
pub fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
