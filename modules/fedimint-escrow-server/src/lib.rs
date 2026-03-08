#![deny(clippy::pedantic)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

use std::collections::BTreeMap;

use async_trait::async_trait;
use fedimint_core::config::{
    ServerModuleConfig, ServerModuleConsensusConfig, TypedServerModuleConfig,
};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::db::{DatabaseTransaction, DatabaseVersion, IDatabaseTransactionOpsCoreTyped};
use fedimint_core::module::audit::Audit;
use fedimint_core::module::{
    Amounts, ApiEndpoint, CORE_CONSENSUS_VERSION, CoreConsensusVersion, InputMeta,
    ModuleConsensusVersion, ModuleInit, SupportedModuleApiVersions, TransactionItemAmounts,
};
use fedimint_core::{Amount, InPoint, OutPoint, PeerId, push_db_pair_items};
pub use fedimint_escrow_common as common;
use fedimint_escrow_common::config::{
    EscrowClientConfig, EscrowConfig, EscrowConfigConsensus, EscrowConfigPrivate,
};
use fedimint_escrow_common::{
    EscrowCommonInit, EscrowConsensusItem, EscrowInput, EscrowInputError, EscrowModuleTypes,
    EscrowOutput, EscrowOutputError, EscrowOutputOutcome, EscrowResolution, EscrowStatus,
    MODULE_CONSENSUS_VERSION,
};
use fedimint_logging::LOG_MODULE_ESCROW;
use fedimint_server_core::config::PeerHandleOps;
use fedimint_server_core::migration::ServerModuleDbMigrationFn;
use fedimint_server_core::{
    ConfigGenModuleArgs, ServerModule, ServerModuleInit, ServerModuleInitArgs,
};
use futures::StreamExt;
use strum::IntoEnumIterator;
use tracing::info;

use crate::db::{DbKeyPrefix, EscrowKey, EscrowPrefix, EscrowRecord};

pub mod db;

/// Module initializer — registered with `fedimintd`.
#[derive(Debug, Clone)]
pub struct EscrowInit;

impl ModuleInit for EscrowInit {
    type Common = EscrowCommonInit;

    async fn dump_database(
        &self,
        dbtx: &mut DatabaseTransaction<'_>,
        prefix_names: Vec<String>,
    ) -> Box<dyn Iterator<Item = (String, Box<dyn erased_serde::Serialize + Send>)> + '_> {
        let mut items: BTreeMap<String, Box<dyn erased_serde::Serialize + Send>> = BTreeMap::new();
        let filtered_prefixes = DbKeyPrefix::iter().filter(|f| {
            prefix_names.is_empty() || prefix_names.contains(&f.to_string().to_lowercase())
        });

        for table in filtered_prefixes {
            match table {
                DbKeyPrefix::Escrow => {
                    push_db_pair_items!(
                        dbtx,
                        EscrowPrefix,
                        EscrowKey,
                        EscrowRecord,
                        items,
                        "Escrow Records"
                    );
                }
            }
        }

        Box::new(items.into_iter())
    }
}

#[async_trait]
impl ServerModuleInit for EscrowInit {
    type Module = Escrow;

    fn versions(&self, _core: CoreConsensusVersion) -> &[ModuleConsensusVersion] {
        &[MODULE_CONSENSUS_VERSION]
    }

    fn supported_api_versions(&self) -> SupportedModuleApiVersions {
        SupportedModuleApiVersions::from_raw(
            (CORE_CONSENSUS_VERSION.major, CORE_CONSENSUS_VERSION.minor),
            (
                MODULE_CONSENSUS_VERSION.major,
                MODULE_CONSENSUS_VERSION.minor,
            ),
            &[(0, 0)],
        )
    }

    async fn init(&self, args: &ServerModuleInitArgs<Self>) -> anyhow::Result<Self::Module> {
        Ok(Escrow::new(args.cfg().to_typed()?))
    }

    fn trusted_dealer_gen(
        &self,
        peers: &[PeerId],
        _args: &ConfigGenModuleArgs,
    ) -> BTreeMap<PeerId, ServerModuleConfig> {
        peers
            .iter()
            .map(|&peer| {
                let config = EscrowConfig {
                    private: EscrowConfigPrivate,
                    consensus: EscrowConfigConsensus,
                };
                (peer, config.to_erased())
            })
            .collect()
    }

    async fn distributed_gen(
        &self,
        _peers: &(dyn PeerHandleOps + Send + Sync),
        _args: &ConfigGenModuleArgs,
    ) -> anyhow::Result<ServerModuleConfig> {
        Ok(EscrowConfig {
            private: EscrowConfigPrivate,
            consensus: EscrowConfigConsensus,
        }
        .to_erased())
    }

    fn get_client_config(
        &self,
        _config: &ServerModuleConsensusConfig,
    ) -> anyhow::Result<EscrowClientConfig> {
        Ok(EscrowClientConfig)
    }

    fn validate_config(
        &self,
        _identity: &PeerId,
        _config: ServerModuleConfig,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn get_database_migrations(
        &self,
    ) -> BTreeMap<DatabaseVersion, ServerModuleDbMigrationFn<Escrow>> {
        BTreeMap::new()
    }
}

/// The escrow module — holds funds in escrow for marketplace orders.
#[derive(Debug)]
pub struct Escrow {
    pub cfg: EscrowConfig,
}

#[async_trait]
impl ServerModule for Escrow {
    type Common = EscrowModuleTypes;
    type Init = EscrowInit;

    async fn consensus_proposal(
        &self,
        _dbtx: &mut DatabaseTransaction<'_>,
    ) -> Vec<EscrowConsensusItem> {
        Vec::new()
    }

    async fn process_consensus_item<'a, 'b>(
        &'a self,
        _dbtx: &mut DatabaseTransaction<'b>,
        _consensus_item: EscrowConsensusItem,
        _peer_id: PeerId,
    ) -> anyhow::Result<()> {
        anyhow::bail!("The escrow module does not use consensus items");
    }

    /// Process an escrow input (reserve): lock funds for an order.
    ///
    /// The customer's e-cash is consumed and held by the federation.
    /// A new `EscrowRecord` is created with status `Reserved`.
    async fn process_input<'a, 'b, 'c>(
        &'a self,
        dbtx: &mut DatabaseTransaction<'c>,
        input: &'b EscrowInput,
        _in_point: InPoint,
    ) -> Result<InputMeta, EscrowInputError> {
        if input.amount == Amount::ZERO {
            return Err(EscrowInputError::ZeroAmount);
        }

        // Check for existing escrow on this order
        let existing = dbtx.get_value(&EscrowKey(input.order_id.clone())).await;
        if let Some(record) = existing {
            if record.status == EscrowStatus::Reserved {
                return Err(EscrowInputError::AlreadyExists(input.order_id.clone()));
            }
        }

        // Create the escrow record
        let record = EscrowRecord {
            amount: input.amount,
            customer_key: input.customer_key,
            status: EscrowStatus::Reserved,
        };
        dbtx.insert_entry(&EscrowKey(input.order_id.clone()), &record)
            .await;

        info!(
            target: LOG_MODULE_ESCROW,
            order_id = %input.order_id,
            amount = %input.amount,
            "Escrow reserved"
        );

        Ok(InputMeta {
            amount: TransactionItemAmounts {
                amounts: Amounts::new_bitcoin(input.amount),
                fees: Amounts::ZERO,
            },
            pub_key: input.customer_key,
        })
    }

    /// Process an escrow output (fulfil or cancel): release held funds.
    ///
    /// - **Fulfil**: pays the supplier (output amount goes to supplier's
    ///   e-cash)
    /// - **Cancel**: refunds the customer (output amount goes back to
    ///   customer's e-cash)
    async fn process_output<'a, 'b>(
        &'a self,
        dbtx: &mut DatabaseTransaction<'b>,
        output: &'a EscrowOutput,
        _out_point: OutPoint,
    ) -> Result<TransactionItemAmounts, EscrowOutputError> {
        let record = dbtx
            .get_value(&EscrowKey(output.order_id.clone()))
            .await
            .ok_or_else(|| EscrowOutputError::NotFound(output.order_id.clone()))?;

        if record.status != EscrowStatus::Reserved {
            return Err(EscrowOutputError::AlreadyResolved(
                output.order_id.clone(),
                record.status,
            ));
        }

        let new_status = match &output.resolution {
            EscrowResolution::Fulfil { .. } => EscrowStatus::Fulfilled,
            EscrowResolution::Cancel => EscrowStatus::Cancelled,
        };

        // Update the escrow record
        let updated = EscrowRecord {
            status: new_status.clone(),
            ..record.clone()
        };
        dbtx.insert_entry(&EscrowKey(output.order_id.clone()), &updated)
            .await;

        info!(
            target: LOG_MODULE_ESCROW,
            order_id = %output.order_id,
            amount = %record.amount,
            status = ?new_status,
            "Escrow resolved"
        );

        Ok(TransactionItemAmounts {
            amounts: Amounts::new_bitcoin(record.amount),
            fees: Amounts::ZERO,
        })
    }

    async fn output_status(
        &self,
        _dbtx: &mut DatabaseTransaction<'_>,
        _out_point: OutPoint,
    ) -> Option<EscrowOutputOutcome> {
        // For now we don't track by out_point — clients query via API endpoint.
        // This could be extended to map out_point → order_id.
        None
    }

    async fn audit(
        &self,
        dbtx: &mut DatabaseTransaction<'_>,
        audit: &mut Audit,
        module_instance_id: ModuleInstanceId,
    ) {
        // Reserved escrows are liabilities — the federation owes these funds
        // (either to supplier on fulfil or to customer on cancel).
        audit
            .add_items(dbtx, module_instance_id, &EscrowPrefix, |_, record| {
                match record.status {
                    EscrowStatus::Reserved => -(record.amount.msats as i64),
                    // Fulfilled and cancelled escrows are settled — no liability.
                    EscrowStatus::Fulfilled | EscrowStatus::Cancelled => 0,
                }
            })
            .await;
    }

    fn api_endpoints(&self) -> Vec<ApiEndpoint<Self>> {
        vec![
            // GET /escrow/:order_id — query escrow status
            // TODO: implement via fedimint API endpoint macro
        ]
    }
}

impl Escrow {
    pub fn new(cfg: EscrowConfig) -> Self {
        Self { cfg }
    }
}
