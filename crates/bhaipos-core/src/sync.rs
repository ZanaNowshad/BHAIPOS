use crate::{BranchId, DeviceId, OperationId, TenantId};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncMutationMaterial {
    pub mutation_id: Uuid,
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub device_id: DeviceId,
    pub operation_id: OperationId,
    pub entity_type: String,
    pub entity_id: String,
    pub mutation_type: String,
    pub payload_json: String,
    pub payload_sha256: String,
    pub credential_version: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncMutationEnvelope {
    pub material: SyncMutationMaterial,
    pub signature_hex: String,
    pub lease_token: Uuid,
}

pub fn sign_sync_mutation(secret: &[u8], material: &SyncMutationMaterial) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key size");
    mac.update(&serde_json::to_vec(material).expect("serializable sync material"));
    hex::encode(mac.finalize().into_bytes())
}

pub fn verify_sync_mutation(
    secret: &[u8],
    material: &SyncMutationMaterial,
    signature_hex: &str,
) -> bool {
    let Ok(signature) = hex::decode(signature_hex) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(&serde_json::to_vec(material).unwrap_or_default());
    mac.verify_slice(&signature).is_ok()
}
