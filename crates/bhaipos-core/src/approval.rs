use crate::{BranchId, DeviceId, OperationId, TenantId, UserId};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalBinding {
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub approver_user_id: UserId,
    pub device_id: DeviceId,
    pub operation_id: OperationId,
    pub action: String,
    pub entity_id: String,
    pub payload_sha256: String,
    pub nonce: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub fn sign_approval(secret: &[u8], binding: &ApprovalBinding) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key size");
    mac.update(&serde_json::to_vec(binding).expect("serializable binding"));
    hex::encode(mac.finalize().into_bytes())
}

pub fn verify_approval(
    secret: &[u8],
    binding: &ApprovalBinding,
    signature_hex: &str,
    now: DateTime<Utc>,
) -> bool {
    if now < binding.issued_at
        || now > binding.expires_at
        || binding.expires_at <= binding.issued_at
        || binding.expires_at - binding.issued_at > chrono::Duration::minutes(5)
    {
        return false;
    }
    let sig = match hex::decode(signature_hex) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut mac = match Hmac::<Sha256>::new_from_slice(secret) {
        Ok(v) => v,
        Err(_) => return false,
    };
    mac.update(&serde_json::to_vec(binding).unwrap_or_default());
    mac.verify_slice(&sig).is_ok()
}
