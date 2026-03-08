use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::{impl_db_lookup, impl_db_record};
use fedimint_escrow_common::EscrowStatus;
use serde::Serialize;
use strum_macros::EnumIter;

/// Client-side DB key prefixes.
#[repr(u8)]
#[derive(Clone, EnumIter, Debug)]
pub enum DbKeyPrefix {
    /// Tracks escrows this client has initiated.
    ActiveEscrow = 0x01,
    #[allow(dead_code)]
    ExternalReservedStart = 0xb0,
    #[allow(dead_code)]
    CoreInternalReservedStart = 0xd0,
    #[allow(dead_code)]
    CoreInternalReservedEnd = 0xff,
}

impl std::fmt::Display for DbKeyPrefix {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Client-side record of an escrow this client initiated.
#[derive(Debug, Clone, Encodable, Decodable, Serialize)]
pub struct ClientEscrowRecord {
    pub amount: fedimint_core::Amount,
    pub status: EscrowStatus,
}

/// DB key: escrow by order ID (client-side).
#[derive(Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize)]
pub struct ClientEscrowKey(pub String);

/// Prefix for scanning all client escrows.
#[derive(Debug, Encodable, Decodable)]
pub struct ClientEscrowPrefix;

impl_db_record!(
    key = ClientEscrowKey,
    value = ClientEscrowRecord,
    db_prefix = DbKeyPrefix::ActiveEscrow,
);
impl_db_lookup!(key = ClientEscrowKey, query_prefix = ClientEscrowPrefix);
