# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.16.2](https://github.com/alloy-rs/evm/releases/tag/v0.16.2) - 2025-07-30

### Features

- More impls for `Either` ([#147](https://github.com/alloy-rs/evm/issues/147))

## [0.16.1](https://github.com/alloy-rs/evm/releases/tag/v0.16.1) - 2025-07-28

### Bug Fixes

- Handle precompile reverts in `PrecompilesMap` ([#144](https://github.com/alloy-rs/evm/issues/144))

### Miscellaneous Tasks

- Release 0.16.1

## [0.16.0](https://github.com/alloy-rs/evm/releases/tag/v0.16.0) - 2025-07-27

### Dependencies

- Bump msrv ([#142](https://github.com/alloy-rs/evm/issues/142))
- Bump revm2710 ([#141](https://github.com/alloy-rs/evm/issues/141))

### Features

- Add as_invalid_tx_err in InvalidTxError ([#143](https://github.com/alloy-rs/evm/issues/143))

### Miscellaneous Tasks

- Release 0.16.0

## [0.15.0](https://github.com/alloy-rs/evm/releases/tag/v0.15.0) - 2025-07-21

### Dependencies

- Bump revm 2703 ([#133](https://github.com/alloy-rs/evm/issues/133))

### Features

- Add Any bound and as_any method to InvalidTxError trait ([#128](https://github.com/alloy-rs/evm/issues/128))
- Enhance precompile lookup documentation and functionality ([#137](https://github.com/alloy-rs/evm/issues/137))
- `EvmInternals::log` ([#135](https://github.com/alloy-rs/evm/issues/135))
- Make fusing optional in `TxTracer` ([#131](https://github.com/alloy-rs/evm/issues/131))
- Add is_pure method to Precompile trait ([#126](https://github.com/alloy-rs/evm/issues/126))

### Miscellaneous Tasks

- Release 0.15.0
- Convert PrecompilesMap into struct ([#136](https://github.com/alloy-rs/evm/issues/136))
- Use revm system_call ([#121](https://github.com/alloy-rs/evm/issues/121))
- Reorder struct def ([#129](https://github.com/alloy-rs/evm/issues/129))

### Performance

- Allocate capacity for balance changes ([#139](https://github.com/alloy-rs/evm/issues/139))
- Only fuse inspector once ([#134](https://github.com/alloy-rs/evm/issues/134))

## [0.14.0](https://github.com/alloy-rs/evm/releases/tag/v0.14.0) - 2025-07-03

### Dependencies

- Bump revm 27.0.2 ([#124](https://github.com/alloy-rs/evm/issues/124))

### Features

- Add BlockEnv to EvmInternals ([#123](https://github.com/alloy-rs/evm/issues/123))

### Miscellaneous Tasks

- Release 0.14.0

## [0.13.0](https://github.com/alloy-rs/evm/releases/tag/v0.13.0) - 2025-07-01

### Dependencies

- Bump revm ([#122](https://github.com/alloy-rs/evm/issues/122))

### Features

- Add object-safe EvmInternals trait for journal operations ([#118](https://github.com/alloy-rs/evm/issues/118))
- Add builder-style methods for precompile manipulation ([#120](https://github.com/alloy-rs/evm/issues/120))
- Add setter utils to blockenv ([#116](https://github.com/alloy-rs/evm/issues/116))

### Miscellaneous Tasks

- Release 0.13.0
- Add some DynPrompile impls ([#117](https://github.com/alloy-rs/evm/issues/117))

## [0.12.3](https://github.com/alloy-rs/evm/releases/tag/v0.12.3) - 2025-06-24

### Bug Fixes

- `the trait bound `[u8]: AsRef<[_; 0]>` is not satisfied` in `precompiles` ([#114](https://github.com/alloy-rs/evm/issues/114))

### Miscellaneous Tasks

- Release 0.12.3

## [0.12.2](https://github.com/alloy-rs/evm/releases/tag/v0.12.2) - 2025-06-23

### Bug Fixes

- Fix compilation

### Documentation

- Fix typo in tracing.rs comment ([#113](https://github.com/alloy-rs/evm/issues/113))
- Correct spelling of "commit" in comments ([#112](https://github.com/alloy-rs/evm/issues/112))

### Features

- Add call-util feature with caller_gas_allowance ([#111](https://github.com/alloy-rs/evm/issues/111))

### Miscellaneous Tasks

- Release 0.12.2
- Derive Copy for `PrecompileInput` ([#110](https://github.com/alloy-rs/evm/issues/110))
- Release 0.12.1

## [0.12.0](https://github.com/alloy-rs/evm/releases/tag/v0.12.0) - 2025-06-20

### Dependencies

- Bump revm 25 ([#100](https://github.com/alloy-rs/evm/issues/100))

### Documentation

- Improve apply_precompile documentation ([#106](https://github.com/alloy-rs/evm/issues/106))
- Improve BlockExecutorFactory and ExecutionCtx documentation ([#104](https://github.com/alloy-rs/evm/issues/104))
- Improve transaction trait documentation ([#103](https://github.com/alloy-rs/evm/issues/103))

### Features

- Add RPC utilities for block and state overrides ([#108](https://github.com/alloy-rs/evm/issues/108))
- Provide more context to `Precompile::call` ([#109](https://github.com/alloy-rs/evm/issues/109))

### Miscellaneous Tasks

- Release 0.12.0

## [0.11.0](https://github.com/alloy-rs/evm/releases/tag/v0.11.0) - 2025-06-11

### Features

- Tracing helpers ([#89](https://github.com/alloy-rs/evm/issues/89))

### Miscellaneous Tasks

- Release 0.11.0
- Update `op-alloy-consensus` ([#101](https://github.com/alloy-rs/evm/issues/101))

## [0.10.0](https://github.com/alloy-rs/evm/releases/tag/v0.10.0) - 2025-05-23

### Dependencies

- [`deps`] Bump revm to `24.0.0` and op-revm to `5.0.0` ([#98](https://github.com/alloy-rs/evm/issues/98))

### Features

- Implement from_recovered_tx for txDeposit nativel ([#96](https://github.com/alloy-rs/evm/issues/96))

### Miscellaneous Tasks

- Release 0.10.0
- Preparing for mint nonoptional in reth ([#91](https://github.com/alloy-rs/evm/issues/91))

## [0.9.1](https://github.com/alloy-rs/evm/releases/tag/v0.9.1) - 2025-05-20

### Features

- Implement `FromTxWithEncoded` and `FromRecoveredTx` from `OpTxEnvelope` for `TxEnv` ([#94](https://github.com/alloy-rs/evm/issues/94))

### Miscellaneous Tasks

- Release 0.9.1

## [0.9.0](https://github.com/alloy-rs/evm/releases/tag/v0.9.0) - 2025-05-20

### Features

- Add non-mutable getters for `inspector` and `precompiles` ([#93](https://github.com/alloy-rs/evm/issues/93))
- `BlockExecutor::execute_transaction_with_commit_condition` ([#92](https://github.com/alloy-rs/evm/issues/92))

### Miscellaneous Tasks

- Release 0.9.0

## [0.8.1](https://github.com/alloy-rs/evm/releases/tag/v0.8.1) - 2025-05-16

### Features

- Extend Evm::Spec bounds with Hash and PartialEq ([#88](https://github.com/alloy-rs/evm/issues/88))

### Miscellaneous Tasks

- Release 0.8.1

## [0.8.0](https://github.com/alloy-rs/evm/releases/tag/v0.8.0) - 2025-05-13

### Dependencies

- Bump alloy 1.0.0 ([#87](https://github.com/alloy-rs/evm/issues/87))

### Miscellaneous Tasks

- Release 0.8.0

## [0.7.2](https://github.com/alloy-rs/evm/releases/tag/v0.7.2) - 2025-05-12

### Bug Fixes

- `r.as_ref()` the trait `AsRef<[_; 0]>` is not implemented for `[u8]` ([#86](https://github.com/alloy-rs/evm/issues/86))

### Miscellaneous Tasks

- Release 0.7.2

### Styling

- Impl Evm for Either ([#84](https://github.com/alloy-rs/evm/issues/84))

## [0.7.1](https://github.com/alloy-rs/evm/releases/tag/v0.7.1) - 2025-05-09

### Dependencies

- Bump op-revm ([#85](https://github.com/alloy-rs/evm/issues/85))

### Miscellaneous Tasks

- Release 0.7.1

## [0.7.0](https://github.com/alloy-rs/evm/releases/tag/v0.7.0) - 2025-05-08

### Bug Fixes

- Use HashMap::with_capacity_and_hasher ([#83](https://github.com/alloy-rs/evm/issues/83))

### Dependencies

- Bump op-revm ([#79](https://github.com/alloy-rs/evm/issues/79))

### Features

- Expose Inspector on Evm ([#81](https://github.com/alloy-rs/evm/issues/81))
- [eip7702] Delegate signer recovery to `alloy-consensus::crypto` ([#82](https://github.com/alloy-rs/evm/issues/82))
- Bump revm ([#74](https://github.com/alloy-rs/evm/issues/74))
- Include Precompiles associated type in Evm trait ([#73](https://github.com/alloy-rs/evm/issues/73))
- Add SpecPrecompiles ([#71](https://github.com/alloy-rs/evm/issues/71))

### Miscellaneous Tasks

- Release 0.7.0
- Use as_ref ([#80](https://github.com/alloy-rs/evm/issues/80))

### Styling

- Re-export revm & op-revm ([#77](https://github.com/alloy-rs/evm/issues/77))

## [0.6.0](https://github.com/alloy-rs/evm/releases/tag/v0.6.0) - 2025-04-23

### Dependencies

- Bump alloy 0.15 ([#72](https://github.com/alloy-rs/evm/issues/72))

### Miscellaneous Tasks

- Release 0.6.0

## [0.5.0](https://github.com/alloy-rs/evm/releases/tag/v0.5.0) - 2025-04-15

### Dependencies

- Bump `op-alloy-consensus` ([#66](https://github.com/alloy-rs/evm/issues/66))
- Bump `op-revm` to `3.0.1` ([#65](https://github.com/alloy-rs/evm/issues/65))

### Features

- Added method to get chain id ([#62](https://github.com/alloy-rs/evm/issues/62))

### Miscellaneous Tasks

- Release 0.5.0

## [0.4.0](https://github.com/alloy-rs/evm/releases/tag/v0.4.0) - 2025-04-09

### Dependencies

- Alloy 0.14 ([#63](https://github.com/alloy-rs/evm/issues/63))

### Miscellaneous Tasks

- Release 0.4.0

## [0.3.2](https://github.com/alloy-rs/evm/releases/tag/v0.3.2) - 2025-04-08

### Features

- Add fn evm(&self) ([#60](https://github.com/alloy-rs/evm/issues/60))

### Miscellaneous Tasks

- Release 0.3.2

## [0.3.1](https://github.com/alloy-rs/evm/releases/tag/v0.3.1) - 2025-04-02

### Features

- Add missing trait impls for ref types ([#58](https://github.com/alloy-rs/evm/issues/58))

### Miscellaneous Tasks

- Release 0.3.1

## [0.3.0](https://github.com/alloy-rs/evm/releases/tag/v0.3.0) - 2025-04-02

### Features

- [tx] Add `FromTxWithEncoded` bound to `BlockExecutor` transaction ([#54](https://github.com/alloy-rs/evm/issues/54))
- [tx] Relax bounds on `TxEip4844` for `EthereumTxEnvelope` ([#57](https://github.com/alloy-rs/evm/issues/57))
- [tx] Implement `FromTxWithEncoded` and `FromRecoveredTx` for `EthereumTxEnvelope` ([#56](https://github.com/alloy-rs/evm/issues/56))

### Miscellaneous Tasks

- Release 0.3.0

### Other

- Rm precise pin ([#55](https://github.com/alloy-rs/evm/issues/55))
- Added execute_block ([#50](https://github.com/alloy-rs/evm/issues/50))

## [0.2.0](https://github.com/alloy-rs/evm/releases/tag/v0.2.0) - 2025-03-28

### Dependencies

- Bump deps revm alloy ([#48](https://github.com/alloy-rs/evm/issues/48))

### Features

- Add helper trait for deriving `TxEnv` from `WithEncoded` ([#42](https://github.com/alloy-rs/evm/issues/42))
- [op-receipt-builder] Add Debug trait to OpReceiptBuilder. ([#47](https://github.com/alloy-rs/evm/issues/47))

### Miscellaneous Tasks

- Release 0.2.0

<!-- generated by git-cliff -->
