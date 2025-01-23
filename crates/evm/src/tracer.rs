use alloy_primitives::{Address, Bytes, Log, B256, U256};
use revm::Database;

pub struct TracerCtx<'a, DB: Database, TxEnv> {
    pub db: &'a DB,
    pub tx_env: &'a TxEnv,
}

pub struct StepCtx {
    pub current_opcode: u8,
}

struct CallInputs {
    /// The call data of the call.
    pub input: Bytes,
    /// The gas limit of the call.
    pub gas_limit: u64,
    /// The account address of bytecode that is going to be executed.
    pub bytecode_address: Address,
    /// Target address, this account storage is going to be modified.
    pub target_address: Address,
    /// This caller is invoking the call.
    pub caller: Address,
    /// Call value.
    pub value: U256,
}

struct CreateInputs {
    /// Caller address of the EVM.
    pub caller: Address,
    /// Salt, if this is a CREATE2 operation.
    pub salt: Option<B256>,
    /// The value to transfer.
    pub value: U256,
    /// The init code of the contract.
    pub init_code: Bytes,
    /// The gas limit of the call.
    pub gas_limit: u64,
}

enum FrameInput {
    Call(CallInputs),
    Create(CreateInputs),
}

struct FrameOutput {
    /// Whether the call was successful.
    pub is_success: bool,
    /// The output of the instruction execution.
    pub output: Bytes,
    /// The gas used.
    pub gas_used: u64,
    /// Address of the contract, if it was a CREATE frame.
    pub address: Option<Address>,
}

/// EVM tracer.
pub trait Tracer<DB: Database, TxEnv> {
    /// Called when a new frame is created.
    #[inline]
    fn frame(&mut self, frame: FrameInput, context: TracerCtx<'_, DB, TxEnv>) {
        let _ = frame;
        let _ = context;
    }

    /// Called at the end of a frame.
    #[inline]
    fn frame_end(&mut self, frame: FrameOutput, context: TracerCtx<'_, DB, TxEnv>) {
        let _ = frame;
        let _ = context;
    }

    /// Called on each step of the interpreter.
    ///
    /// Information about the current execution, including the memory, stack and more is available
    /// on `interp` (see [Interpreter]).
    ///
    /// # Example
    ///
    /// To get the current opcode, use `interp.current_opcode()`.
    #[inline]
    fn step(&mut self, step: StepCtx, context: TracerCtx<'_, DB, TxEnv>) {
        let _ = step;
        let _ = context;
    }

    /// Called after `step` when the instruction has been executed.
    ///
    /// Setting `interp.instruction_result` to anything other than
    /// [crate::interpreter::InstructionResult::Continue] alters the execution
    /// of the interpreter.
    #[inline]
    fn step_end(&mut self, step: StepCtx, context: TracerCtx<'_, DB, TxEnv>) {
        let _ = step;
        let _ = context;
    }

    /// Called when a log is emitted.
    #[inline]
    fn log(&mut self, context: TracerCtx<'_, DB, TxEnv>, log: &Log) {
        let _ = context;
        let _ = log;
    }

    /// Called when a contract has been self-destructed with funds transferred to target.
    #[inline]
    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        let _ = contract;
        let _ = target;
        let _ = value;
    }
}
