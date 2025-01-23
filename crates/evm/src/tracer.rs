use alloy_primitives::{Address, Bytes, Log, B256, U256};
use revm::{interpreter::InterpreterResult, primitives::TxEnv, Database};

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
    pub salt: Option<U256>,
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
    fn frame_end(
        &mut self,
        input: FrameInput,
        output: FrameOutput,
        context: TracerCtx<'_, DB, TxEnv>,
    ) {
        let _ = input;
        let _ = output;
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

impl From<&revm::interpreter::CallInputs> for FrameInput {
    fn from(value: &revm::interpreter::CallInputs) -> Self {
        let revm::interpreter::CallInputs {
            input,
            gas_limit,
            bytecode_address,
            target_address,
            caller,
            value,
            ..
        } = value;
        FrameInput::Call(CallInputs {
            input: input.clone(),
            gas_limit: *gas_limit,
            bytecode_address: *bytecode_address,
            target_address: *target_address,
            caller: *caller,
            value: value.get(),
        })
    }
}

impl From<&revm::interpreter::CreateInputs> for FrameInput {
    fn from(value: &revm::interpreter::CreateInputs) -> Self {
        let revm::interpreter::CreateInputs { caller, value, init_code, gas_limit, scheme } = value;
        let salt = if let revm::interpreter::CreateScheme::Create2 { salt } = scheme {
            Some(*salt)
        } else {
            None
        };
        FrameInput::Create(CreateInputs {
            caller: *caller,
            salt,
            value: *value,
            init_code: init_code.clone(),
            gas_limit: *gas_limit,
        })
    }
}

impl From<&revm::interpreter::EOFCreateInputs> for FrameInput {
    fn from(value: &revm::interpreter::EOFCreateInputs) -> Self {
        let revm::interpreter::EOFCreateInputs { caller, value, gas_limit, kind } = value;
        let init_code = match kind {
            revm::interpreter::EOFCreateKind::Tx { initdata } => initdata.clone(),
            revm::interpreter::EOFCreateKind::Opcode { initcode, input, .. } => {
                initcode.raw().clone()
            }
        };
        FrameInput::Create(CreateInputs {
            caller: *caller,
            salt: None,
            value: *value,
            init_code,
            gas_limit: *gas_limit,
        })
    }
}

impl From<&revm::interpreter::CallOutcome> for FrameOutput {
    fn from(value: &revm::interpreter::CallOutcome) -> Self {
        let revm::interpreter::CallOutcome {
            result: InterpreterResult { output, result, .. }, ..
        } = value;

        Self { is_success: result.is_ok(), output: output.clone(), address: None }
    }
}

impl From<&revm::interpreter::CreateOutcome> for FrameOutput {
    fn from(value: &revm::interpreter::CreateOutcome) -> Self {
        let revm::interpreter::CreateOutcome {
            result: InterpreterResult { output, result, .. },
            address,
            ..
        } = value;

        Self { is_success: result.is_ok(), output: output.clone(), address: *address }
    }
}

/// [`revm::Inspector`] implementation for [`Tracer`].
struct TracerInspector<T>(T);

impl<T> TracerInspector<T> {
    fn to_step_context(&self, interp: &revm::interpreter::Interpreter) -> StepCtx {
        StepCtx { current_opcode: interp.current_opcode() }
    }

    fn to_tracer_context<'a, DB: Database>(
        &self,
        evm_context: &'a revm::EvmContext<DB>,
    ) -> TracerCtx<'a, DB, TxEnv> {
        TracerCtx { db: &evm_context.db, tx_env: &evm_context.env.tx }
    }
}

impl<DB, T> revm::Inspector<DB> for TracerInspector<T>
where
    DB: Database,
    T: Tracer<DB, TxEnv>,
{
    fn step(
        &mut self,
        interp: &mut revm::interpreter::Interpreter,
        context: &mut revm::EvmContext<DB>,
    ) {
        self.0.step(self.to_step_context(interp), self.to_tracer_context(&context))
    }

    fn step_end(
        &mut self,
        interp: &mut revm::interpreter::Interpreter,
        context: &mut revm::EvmContext<DB>,
    ) {
        self.0.step_end(self.to_step_context(interp), self.to_tracer_context(&context))
    }

    fn call(
        &mut self,
        context: &mut revm::EvmContext<DB>,
        inputs: &mut revm::interpreter::CallInputs,
    ) -> Option<revm::interpreter::CallOutcome> {
        self.0.frame((&*inputs).into(), self.to_tracer_context(&context));
        None
    }

    fn create(
        &mut self,
        context: &mut revm::EvmContext<DB>,
        inputs: &mut revm::interpreter::CreateInputs,
    ) -> Option<revm::interpreter::CreateOutcome> {
        self.0.frame((&*inputs).into(), self.to_tracer_context(&context));
        None
    }

    fn eofcreate(
        &mut self,
        context: &mut revm::EvmContext<DB>,
        inputs: &mut revm::interpreter::EOFCreateInputs,
    ) -> Option<revm::interpreter::CreateOutcome> {
        self.0.frame((&*inputs).into(), self.to_tracer_context(&context));
        None
    }

    fn call_end(
        &mut self,
        context: &mut revm::EvmContext<DB>,
        inputs: &revm::interpreter::CallInputs,
        outcome: revm::interpreter::CallOutcome,
    ) -> revm::interpreter::CallOutcome {
        self.0.frame_end(inputs.into(), (&outcome).into(), self.to_tracer_context(&context));
        outcome
    }

    fn create_end(
        &mut self,
        context: &mut revm::EvmContext<DB>,
        inputs: &revm::interpreter::CreateInputs,
        outcome: revm::interpreter::CreateOutcome,
    ) -> revm::interpreter::CreateOutcome {
        self.0.frame_end(inputs.into(), (&outcome).into(), self.to_tracer_context(&context));
        outcome
    }

    fn eofcreate_end(
        &mut self,
        context: &mut revm::EvmContext<DB>,
        inputs: &revm::interpreter::EOFCreateInputs,
        outcome: revm::interpreter::CreateOutcome,
    ) -> revm::interpreter::CreateOutcome {
        self.0.frame_end(inputs.into(), (&outcome).into(), self.to_tracer_context(&context));
        outcome
    }

    fn log(
        &mut self,
        interp: &mut revm::interpreter::Interpreter,
        context: &mut revm::EvmContext<DB>,
        log: &Log,
    ) {
        self.0.log(self.to_tracer_context(&context), log);
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        self.0.selfdestruct(contract, target, value);
    }
}
