use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditMaterial<'a> {
    pub tenant_id: &'a str,
    pub device_id: &'a str,
    pub actor_user_id: &'a str,
    pub event_type: &'a str,
    pub entity_type: &'a str,
    pub entity_id: &'a str,
    pub payload_json: &'a str,
    pub created_at: DateTime<Utc>,
    pub previous_hash: &'a str,
}

pub fn compute_audit_hash(material: &AuditMaterial<'_>) -> String {
    let bytes = serde_json::to_vec(material).expect("serializable audit material");
    hex::encode(Sha256::digest(bytes))
}

pub fn sha256_hex(bytes: &[u8]) -> String { hex::encode(Sha256::digest(bytes)) }
