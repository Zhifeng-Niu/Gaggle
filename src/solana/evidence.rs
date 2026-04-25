//! 证据上链模块
//!
//! MVP版本：生成模拟的tx signature
//! 未来演进：真实的Solana程序调用

use serde::{Deserialize, Serialize};
use sha2::Digest;

/// 证据类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvidenceType {
    /// 会话Hash（用于验证谈判记录完整性）
    SessionHash,
    /// 最终协议（成交结果）
    FinalAgreement,
}

/// 证据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Space ID
    pub space_id: String,
    /// 证据类型
    pub evidence_type: EvidenceType,
    /// SHA-256 Hash
    pub hash: String,
    /// 创建时间戳
    pub created_at: i64,
    /// Solana交易签名
    pub tx_signature: Option<String>,
    /// Solana Slot
    pub slot: Option<u64>,
}

impl Evidence {
    /// 创建新证据
    pub fn new(space_id: String, evidence_type: EvidenceType, hash: String) -> Self {
        Self {
            space_id,
            evidence_type,
            hash,
            created_at: chrono::Utc::now().timestamp_millis(),
            tx_signature: None,
            slot: None,
        }
    }
}

/// 提交证据到链上（MVP版本为模拟实现）
pub fn submit_evidence(
    _space_id: &str,
    _evidence_type: &EvidenceType,
    hash: &str,
) -> Result<(String, u64), crate::error::GaggleError> {
    // MVP版本：生成模拟的tx signature
    // 未来演进：调用真实的Solana程序

    let tx_signature = format!(
        "simulated_{}_{}",
        &hash[..16.min(hash.len())],
        chrono::Utc::now().timestamp_millis()
    );
    let slot = 123456789u64;

    tracing::info!(
        "Evidence submitted (simulated): space_id={}, hash={}, tx={}",
        _space_id,
        hash,
        tx_signature
    );

    Ok((tx_signature, slot))
}

/// 验证证据Hash
pub fn verify_hash(data: &str, expected_hash: &str) -> bool {
    let mut hasher = sha2::Sha256::new();
    hasher.update(data.as_bytes());
    let computed = format!("{:x}", hasher.finalize());
    computed == expected_hash
}

/// 从谈判记录计算Session Hash
pub fn compute_session_hash(messages: &[crate::negotiation::SpaceMessage]) -> String {
    let mut hasher = sha2::Sha256::new();

    for msg in messages {
        hasher.update(msg.id.as_bytes());
        hasher.update(msg.space_id.as_bytes());
        hasher.update(msg.sender_id.as_bytes());
        hasher.update(format!("{:?}", msg.msg_type).as_bytes());
        hasher.update(msg.content.cipher.as_bytes());
        hasher.update(msg.content.nonce.as_bytes());
        hasher.update(msg.timestamp.to_le_bytes());
        hasher.update(msg.round.to_le_bytes());
    }

    format!("{:x}", hasher.finalize())
}
