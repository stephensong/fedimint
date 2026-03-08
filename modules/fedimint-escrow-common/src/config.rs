use fedimint_core::core::ModuleKind;
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::plugin_types_trait_impl_config;
use serde::{Deserialize, Serialize};

use crate::EscrowCommonInit;

/// Server-side configuration (private + consensus).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscrowConfig {
    pub private: EscrowConfigPrivate,
    pub consensus: EscrowConfigConsensus,
}

/// Client-visible configuration (distributed during federation join).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Encodable, Decodable, Hash)]
pub struct EscrowClientConfig;

/// Consensus configuration — shared identically across all federation members.
#[derive(Clone, Debug, Serialize, Deserialize, Decodable, Encodable)]
pub struct EscrowConfigConsensus;

/// Private configuration — encrypted per-peer, not shared.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscrowConfigPrivate;

plugin_types_trait_impl_config!(
    EscrowCommonInit,
    EscrowConfig,
    EscrowConfigPrivate,
    EscrowConfigConsensus,
    EscrowClientConfig
);
