use fedimint_client_module::DynGlobalClientContext;
use fedimint_client_module::sm::{DynState, State, StateTransition};
use fedimint_core::core::{IntoDynInstance, ModuleInstanceId, OperationId};
use fedimint_core::encoding::{Decodable, Encodable};

use crate::EscrowClientContext;

/// Client-side state machine for escrow operations.
///
/// Currently empty — escrow operations are single-step transactions.
/// State machines will be added when we need to track in-flight
/// reserve/fulfil/cancel operations with retry logic.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Decodable, Encodable)]
pub enum EscrowStateMachine {}

impl State for EscrowStateMachine {
    type ModuleContext = EscrowClientContext;

    fn transitions(
        &self,
        _context: &Self::ModuleContext,
        _global_context: &DynGlobalClientContext,
    ) -> Vec<StateTransition<Self>> {
        unreachable!()
    }

    fn operation_id(&self) -> OperationId {
        unreachable!()
    }
}

impl IntoDynInstance for EscrowStateMachine {
    type DynType = DynState;

    fn into_dyn(self, instance_id: ModuleInstanceId) -> Self::DynType {
        DynState::from_typed(instance_id, self)
    }
}
