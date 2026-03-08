#![deny(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

use std::fmt;

use config::EscrowClientConfig;
use fedimint_core::core::{Decoder, ModuleInstanceId, ModuleKind};
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::module::{CommonModuleInit, ModuleCommon, ModuleConsensusVersion};
use fedimint_core::{Amount, plugin_types_trait_impl_common};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod config;

/// Unique name for this module
pub const KIND: ModuleKind = ModuleKind::from_static_str("escrow");

/// Module consensus version
pub const MODULE_CONSENSUS_VERSION: ModuleConsensusVersion = ModuleConsensusVersion::new(0, 1);

/// Escrow status for an order.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Encodable, Decodable)]
pub enum EscrowStatus {
    /// Funds are held in escrow, awaiting fulfilment or cancellation.
    Reserved,
    /// Supplier has been paid — order fulfilled.
    Fulfilled,
    /// Customer has been refunded — order cancelled.
    Cancelled,
}

/// Non-transaction items submitted to consensus (not used currently).
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Encodable, Decodable)]
pub struct EscrowConsensusItem;

/// Input: customer deposits funds into escrow for an order.
///
/// This locks `amount` from the customer's e-cash balance into the
/// federation-held escrow, keyed by `order_id`.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Deserialize, Serialize, Encodable, Decodable)]
pub struct EscrowInput {
    /// The order this escrow relates to.
    pub order_id: String,
    /// Amount to lock in escrow.
    pub amount: Amount,
    /// Customer's public key (for refund authorization).
    pub customer_key: fedimint_core::secp256k1::PublicKey,
}

/// Output: release escrowed funds (to supplier on fulfil, or back to customer on cancel).
#[derive(Debug, Clone, Eq, PartialEq, Hash, Deserialize, Serialize, Encodable, Decodable)]
pub struct EscrowOutput {
    /// The order being resolved.
    pub order_id: String,
    /// Resolution: fulfil pays the supplier, cancel refunds the customer.
    pub resolution: EscrowResolution,
}

/// How an escrow is resolved.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Deserialize, Serialize, Encodable, Decodable)]
pub enum EscrowResolution {
    /// Pay the supplier — order fulfilled successfully.
    Fulfil {
        /// Supplier's public key (receives the funds).
        supplier_key: fedimint_core::secp256k1::PublicKey,
    },
    /// Refund the customer — order cancelled.
    Cancel,
}

/// Outcome of an escrow output (returned to client after processing).
#[derive(Debug, Clone, Eq, PartialEq, Hash, Deserialize, Serialize, Encodable, Decodable)]
pub struct EscrowOutputOutcome {
    pub status: EscrowStatus,
}

/// Errors returned when processing an escrow input (reserve).
#[derive(Debug, Clone, Eq, PartialEq, Hash, Error, Encodable, Decodable)]
pub enum EscrowInputError {
    #[error("Order {0} already has an active escrow")]
    AlreadyExists(String),
    #[error("Escrow amount must be greater than zero")]
    ZeroAmount,
}

/// Errors returned when processing an escrow output (fulfil/cancel).
#[derive(Debug, Clone, Eq, PartialEq, Hash, Error, Encodable, Decodable)]
pub enum EscrowOutputError {
    #[error("No escrow found for order {0}")]
    NotFound(String),
    #[error("Escrow for order {0} is already resolved ({1:?})")]
    AlreadyResolved(String, EscrowStatus),
}

/// Wire type container for the module.
pub struct EscrowModuleTypes;

plugin_types_trait_impl_common!(
    KIND,
    EscrowModuleTypes,
    EscrowClientConfig,
    EscrowInput,
    EscrowOutput,
    EscrowOutputOutcome,
    EscrowConsensusItem,
    EscrowInputError,
    EscrowOutputError
);

#[derive(Debug)]
pub struct EscrowCommonInit;

impl CommonModuleInit for EscrowCommonInit {
    const CONSENSUS_VERSION: ModuleConsensusVersion = MODULE_CONSENSUS_VERSION;
    const KIND: ModuleKind = KIND;

    type ClientConfig = EscrowClientConfig;

    fn decoder() -> Decoder {
        EscrowModuleTypes::decoder_builder().build()
    }
}

impl fmt::Display for EscrowClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EscrowClientConfig")
    }
}

impl fmt::Display for EscrowInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EscrowInput(order={}, amount={})", self.order_id, self.amount)
    }
}

impl fmt::Display for EscrowOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let action = match &self.resolution {
            EscrowResolution::Fulfil { .. } => "fulfil",
            EscrowResolution::Cancel => "cancel",
        };
        write!(f, "EscrowOutput(order={}, {})", self.order_id, action)
    }
}

impl fmt::Display for EscrowOutputOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EscrowOutcome({:?})", self.status)
    }
}

impl fmt::Display for EscrowConsensusItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EscrowConsensusItem")
    }
}
