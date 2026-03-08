#![deny(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

use std::collections::BTreeMap;

use db::{ClientEscrowKey, ClientEscrowPrefix, ClientEscrowRecord, DbKeyPrefix};
use fedimint_client_module::db::ClientModuleMigrationFn;
use fedimint_client_module::module::init::{ClientModuleInit, ClientModuleInitArgs};
use fedimint_client_module::module::recovery::NoModuleBackup;
use fedimint_client_module::module::{ClientContext, ClientModule};
use fedimint_client_module::sm::Context;
use fedimint_core::core::ModuleKind;
use fedimint_core::db::{
    Database, DatabaseTransaction, DatabaseVersion, IDatabaseTransactionOpsCoreTyped,
};
use fedimint_core::module::{Amounts, ApiVersion, ModuleCommon, ModuleInit, MultiApiVersion};
use fedimint_core::{Amount, apply, async_trait_maybe_send, push_db_pair_items};
pub use fedimint_escrow_common as common;
use fedimint_escrow_common::{
    EscrowCommonInit, EscrowInput, EscrowModuleTypes, EscrowOutput, EscrowResolution, EscrowStatus,
};
use fedimint_logging::LOG_CLIENT_MODULE_ESCROW;
use futures::StreamExt;
use strum::IntoEnumIterator;
use tracing::info;

pub mod db;
mod states;

pub use states::EscrowStateMachine;

/// Client module for marketplace escrow operations.
pub struct EscrowClientModule {
    db: Database,
    #[allow(dead_code)]
    client_ctx: ClientContext<Self>,
}

impl std::fmt::Debug for EscrowClientModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EscrowClientModule").finish_non_exhaustive()
    }
}

/// Context for client-side state machines (minimal for now).
#[derive(Clone, Debug)]
pub struct EscrowClientContext;

impl Context for EscrowClientContext {
    const KIND: Option<ModuleKind> = None;
}

#[apply(async_trait_maybe_send!)]
impl ClientModule for EscrowClientModule {
    type Init = EscrowClientInit;
    type Common = EscrowModuleTypes;
    type Backup = NoModuleBackup;
    type ModuleStateMachineContext = EscrowClientContext;
    type States = EscrowStateMachine;

    fn context(&self) -> Self::ModuleStateMachineContext {
        EscrowClientContext
    }

    fn input_fee(
        &self,
        _amount: &Amounts,
        _input: &<Self::Common as ModuleCommon>::Input,
    ) -> Option<Amounts> {
        Some(Amounts::ZERO)
    }

    fn output_fee(
        &self,
        _amount: &Amounts,
        _output: &<Self::Common as ModuleCommon>::Output,
    ) -> Option<Amounts> {
        Some(Amounts::ZERO)
    }
}

impl EscrowClientModule {
    /// Reserve funds in escrow for a marketplace order.
    ///
    /// Creates an `EscrowInput` transaction that locks the specified amount.
    /// The CREAM UI calls this when a customer places an order.
    pub async fn reserve(
        &self,
        order_id: String,
        amount: Amount,
        customer_key: fedimint_core::secp256k1::PublicKey,
    ) -> anyhow::Result<EscrowInput> {
        let mut dbtx = self.db.begin_transaction().await;
        dbtx.insert_entry(
            &ClientEscrowKey(order_id.clone()),
            &ClientEscrowRecord {
                amount,
                status: EscrowStatus::Reserved,
            },
        )
        .await;
        dbtx.commit_tx().await;

        info!(target: LOG_CLIENT_MODULE_ESCROW, order_id = %order_id, amount = %amount, "Escrow reserve prepared");

        Ok(EscrowInput {
            order_id,
            amount,
            customer_key,
        })
    }

    /// Fulfil an escrow — pay the supplier.
    ///
    /// Creates an `EscrowOutput` transaction that releases funds to the
    /// supplier.
    pub async fn fulfil(
        &self,
        order_id: String,
        supplier_key: fedimint_core::secp256k1::PublicKey,
    ) -> anyhow::Result<EscrowOutput> {
        let mut dbtx = self.db.begin_transaction().await;
        if let Some(mut record) = dbtx.get_value(&ClientEscrowKey(order_id.clone())).await {
            record.status = EscrowStatus::Fulfilled;
            dbtx.insert_entry(&ClientEscrowKey(order_id.clone()), &record)
                .await;
        }
        dbtx.commit_tx().await;

        info!(target: LOG_CLIENT_MODULE_ESCROW, order_id = %order_id, "Escrow fulfil prepared");

        Ok(EscrowOutput {
            order_id,
            resolution: EscrowResolution::Fulfil { supplier_key },
        })
    }

    /// Cancel an escrow — refund the customer.
    pub async fn cancel(&self, order_id: String) -> anyhow::Result<EscrowOutput> {
        let mut dbtx = self.db.begin_transaction().await;
        if let Some(mut record) = dbtx.get_value(&ClientEscrowKey(order_id.clone())).await {
            record.status = EscrowStatus::Cancelled;
            dbtx.insert_entry(&ClientEscrowKey(order_id.clone()), &record)
                .await;
        }
        dbtx.commit_tx().await;

        info!(target: LOG_CLIENT_MODULE_ESCROW, order_id = %order_id, "Escrow cancel prepared");

        Ok(EscrowOutput {
            order_id,
            resolution: EscrowResolution::Cancel,
        })
    }

    /// List all escrows this client has initiated.
    pub async fn list_escrows(&self) -> Vec<(String, ClientEscrowRecord)> {
        let mut dbtx = self.db.begin_transaction().await;
        dbtx.find_by_prefix(&ClientEscrowPrefix)
            .await
            .map(|(key, record)| (key.0, record))
            .collect::<Vec<_>>()
            .await
    }
}

#[derive(Debug, Clone)]
pub struct EscrowClientInit;

impl ModuleInit for EscrowClientInit {
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
                DbKeyPrefix::ActiveEscrow => {
                    push_db_pair_items!(
                        dbtx,
                        ClientEscrowPrefix,
                        ClientEscrowKey,
                        ClientEscrowRecord,
                        items,
                        "Escrow Records"
                    );
                }
                DbKeyPrefix::ExternalReservedStart
                | DbKeyPrefix::CoreInternalReservedStart
                | DbKeyPrefix::CoreInternalReservedEnd => {}
            }
        }

        Box::new(items.into_iter())
    }
}

#[apply(async_trait_maybe_send!)]
impl ClientModuleInit for EscrowClientInit {
    type Module = EscrowClientModule;

    fn supported_api_versions(&self) -> MultiApiVersion {
        MultiApiVersion::try_from_iter([ApiVersion { major: 0, minor: 0 }])
            .expect("no version conflicts")
    }

    async fn init(&self, args: &ClientModuleInitArgs<Self>) -> anyhow::Result<Self::Module> {
        Ok(EscrowClientModule {
            db: args.db().clone(),
            client_ctx: args.context(),
        })
    }

    fn get_database_migrations(&self) -> BTreeMap<DatabaseVersion, ClientModuleMigrationFn> {
        BTreeMap::new()
    }
}
