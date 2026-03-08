use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::{Amount, impl_db_lookup, impl_db_record};
use fedimint_escrow_common::EscrowStatus;
use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

/// Namespaces DB keys for the escrow module.
#[repr(u8)]
#[derive(Clone, EnumIter, Debug)]
pub enum DbKeyPrefix {
    /// Active and resolved escrows, keyed by order ID.
    Escrow = 0x01,
}

impl std::fmt::Display for DbKeyPrefix {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Value stored for each escrow.
#[derive(Debug, Clone, Encodable, Decodable, Serialize, Deserialize)]
pub struct EscrowRecord {
    /// Amount held in escrow.
    pub amount: Amount,
    /// Customer's public key (for refund on cancel).
    pub customer_key: fedimint_core::secp256k1::PublicKey,
    /// Current status.
    pub status: EscrowStatus,
}

/// DB key: escrow by order ID.
#[derive(Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize)]
pub struct EscrowKey(pub String);

/// Prefix for scanning all escrows.
#[derive(Debug, Encodable, Decodable)]
pub struct EscrowPrefix;

impl_db_record!(
    key = EscrowKey,
    value = EscrowRecord,
    db_prefix = DbKeyPrefix::Escrow,
);
impl_db_lookup!(key = EscrowKey, query_prefix = EscrowPrefix);
