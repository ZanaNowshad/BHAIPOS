use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);
        impl $name { pub fn new() -> Self { Self(Uuid::new_v4()) } }
        impl Default for $name { fn default() -> Self { Self::new() } }
        impl std::fmt::Display for $name { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { self.0.fmt(f) } }
    };
}

id_type!(TenantId); id_type!(BranchId); id_type!(UserId); id_type!(DeviceId); id_type!(RegisterId);
id_type!(CashSessionId); id_type!(ProductId); id_type!(CartId); id_type!(SaleId); id_type!(PaymentId); id_type!(OperationId);
id_type!(RefundId); id_type!(CustomerId); id_type!(AuditEventId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus { Active, Suspended, Revoked }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CartStatus { Active, Held, CheckingOut, Completed, Cancelled }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TenderKind { Cash, Card, BenefitPay, BankTransfer, CustomerCredit, Custom }
