# Fedimint Architecture Knowledge Base

> Private knowledge base for CREAM integration. Not for upstream PR.
> Based on reading the full `docs/` directory and source code of the fedimint repository.

## Project Overview

Fedimint is a general framework for building **federated financial applications** with no single point of trust. The current implementation provides federated Chaumian e-cash backed by Bitcoin, with deposits/withdrawals via on-chain or Lightning.

- **Consensus**: [AlephBFT](https://docs.rs/aleph-bft/latest/aleph_bft/) — Byzantine fault tolerant, tolerates f < n/3 malicious nodes
- **E-cash**: Threshold blind signatures (BLS12-381 via `crypto/tbs`)
- **Bitcoin**: On-chain peg-in/peg-out via federated multisig
- **Lightning**: Gateway-mediated payments (V1 HTLC interception, V2 HOLD invoices)

## Workspace Structure

~78 member crates. Key directories:

```
fedimint/
├── bin/                          # Binary entry points
├── crypto/                       # aead, derive-secret, hkdf, tbs (threshold blind sigs), tpe
├── db/                           # Database abstraction (key-value, prefix isolation)
├── devimint/                     # Dev federation orchestrator
├── fedimint-core/                # Shared types, crypto, encoding, DB, networking
├── fedimint-server/              # Consensus engine (AlephBFT), module hosting, API
├── fedimint-client/              # Client SDK (builder, state machines, operation log)
├── fedimint-client-module/       # ClientModule trait, state machine abstractions
├── fedimint-cli/                 # CLI wallet (wraps fedimint-client)
├── fedimintd/                    # Federation node daemon (main server binary)
├── gateway/                      # Lightning gateway (multi-federation, LND/LDK)
│   ├── fedimint-gateway-server/
│   ├── fedimint-gateway-client/
│   ├── fedimint-gateway-common/
│   ├── fedimint-gateway-ui/
│   └── fedimint-lightning/       # LN node abstraction
├── modules/                      # Pluggable modules (3-part pattern each)
│   ├── fedimint-mint-{common,client,server}/
│   ├── fedimint-wallet-{common,client,server}/
│   ├── fedimint-ln-{common,client,server}/
│   ├── fedimint-lnv2-{common,client,server}/
│   ├── fedimint-meta-{common,client,server}/
│   ├── fedimint-dummy-{common,client,server}/   # Example module
│   └── fedimint-empty-{common,client,server}/   # Minimal scaffold
├── fedimint-client-wasm/         # WASM build (wasm-bindgen)
├── fedimint-client-uniffi/       # UniFFI bindings (mobile FFI)
├── fedimint-bip39/               # BIP39 mnemonic support
├── fedimint-bitcoind/            # Bitcoin RPC integration
├── fedimint-recoverytool/        # Recovery utilities
├── nix/                          # Flake overlays & NixOS modules
└── scripts/                      # Build & test scripts
```

### Key Binaries

| Binary | Crate | Purpose |
|--------|-------|---------|
| `fedimintd` | `fedimintd/` | Federation guardian node — runs consensus + API |
| `fedimint-cli` | `fedimint-cli/` | User wallet CLI |
| `gatewayd` | `gateway/fedimint-gateway-server/` | Lightning gateway daemon |
| `gateway-cli` | `gateway/` | Gateway operator CLI |
| `devimint` | `devimint/` | Test orchestrator — spawns full federation + bitcoind + LN |

## Core Crates

### fedimint-core

Shared by both client and server. Provides:

- **Types**: `Amount`, `OperationId` (32-byte), `ModuleInstanceId` (u16), `ModuleKind` (string), `TransactionId`, `OutPoint`
- **Encoding**: Custom `Encodable`/`Decodable` traits with consensus hashing (SHA256). Extensible enums via `#[encodable_default]`
- **Tiered amounts**: `Tiered<T>` (per-denomination), `TieredMulti<T>` (multiple per denomination)
- **Module traits**: `ModuleCommon`, `ServerModule`, `ClientModule`
- **Database abstraction**: Key-value with transactions, prefix isolation, optimistic locking
- **Networking**: API client/server scaffolding, invite codes
- **WASM compat**: Task spawning and time operations for `wasm32-unknown-unknown`

### fedimint-server

- **Consensus engine**: AlephBFT integration — epoch-based agreement
- **Module hosting**: Instantiates and manages all server modules
- **API server**: JSON-RPC over HTTP (port 8174) + Iroh (QUIC+DHT)
- **P2P**: Iroh endpoint (port 8173) for inter-guardian communication
- **DB migrations**: Per-module, backwards-compatible

### fedimint-client

- **ClientBuilder**: Composable module registration → `ClientHandle`
- **State machine executor**: Async operations tracked by `OperationId`
- **Operation log**: Chronological history of all operations
- **Backup/recovery**: Deterministic secret derivation + module-specific recovery
- **Meta service**: Federation metadata cache
- **Primary module**: Designated module for change-making and fund prioritization

### fedimint-client-module

Defines the `ClientModule` trait that all client-side modules implement:

- State machine abstractions (`Context`, `DynState`, `StateTransition`)
- `OperationId`-based tracking for long-running operations
- DB transaction patterns for client modules
- Recovery progress monitoring

## Three-Part Module Pattern

Every module follows `<module>-common` / `<module>-client` / `<module>-server`:

```
    fedimintd (server binary)          fedimint-cli (client binary)
         │                                    │
    loads modules                        builds client
         │                                    │
    <module>-server                    <module>-client
         │                                    │
         └──── both depend on ────> <module>-common
                                          │
                                   implements types from
                                          │
                                     fedimint-core
```

- **Common**: Shared types (inputs, outputs, consensus items, config)
- **Client**: `ClientModule` impl, state machines, operation builders, extension traits
- **Server**: `ServerModule` impl, validation, consensus proposals, DB migrations, API endpoints

## Module Deep-Dives

### Mint Module (Chaumian E-Cash) — `modules/fedimint-mint-*`

The heart of CREAM's CURD integration.

**How it works**:
1. User generates a random `Nonce` (secp256k1 public key — unique note ID)
2. User blinds it into a `BlindNonce` (prevents linking issuance to spending)
3. User submits `MintOutput` (blind nonce) in a transaction
4. Federation guardians each contribute a threshold blind signature share
5. Client combines shares into a full `BlindedSignature`
6. Client unblinds to get a `Note` = `Nonce` + threshold signature
7. To spend: submit `MintInput` with the `Note`; federation verifies signature and marks nonce as spent

**Key types**:
- `Note`: User-generated nonce + threshold signature (the e-cash token)
- `Nonce`: secp256k1 public key, unique note identifier
- `BlindNonce`: Blinded message for threshold signing
- `MintInput`: Spend a note (input to transaction)
- `MintOutput`: Request new blind-signed note (output from transaction)
- `MintConsensusItem`: Server consensus contributions (blind signature shares)

**Denominations**: Notes are denominated (powers of 2 in msat). `Tiered<T>` tracks per-denomination data.

**Recovery**: Deterministic via HKDF — derive all nonces from root secret, scan federation for matching notes.

### Wallet Module (Bitcoin On-Chain) — `modules/fedimint-wallet-*`

**Peg-in flow**:
1. Client generates random keypair, tweaks federation's public multisig with public key
2. Client sends BTC to the tweaked address
3. Client creates `PegInProof` (includes tweak key + UTXO proof + signed message)
4. Federation validates proof is in a block and spendable, stores `SpendableUTXO`
5. After `finality_delay` (10 blocks), peg-in is accepted

**Peg-out flow**:
1. Client requests current fees, creates `PegOut` output
2. Federation validates address, fees, UTXO availability
3. Federation generates PSBT, each guardian signs
4. Combined into final transaction, broadcast to Bitcoin network

**Key types**: `PegInProof`, `PegOut`, `SpendableUTXO`, PSBT aggregation

### Lightning Module V1 — `modules/fedimint-ln-*`

- **OutgoingContract**: User locks e-cash funds → gateway pays LN invoice → provides preimage → federation releases funds to gateway
- **IncomingContract**: Gateway locks funds → user provides proof of payment → funds released to user
- HTLC interception pattern

### Lightning Module V2 — `modules/fedimint-lnv2-*`

- HOLD invoices, gateway-created invoices
- Threshold decryption for preimage recovery
- More flexible payment flows

### Meta Module — `modules/fedimint-meta-*`

- Federation metadata: name, expiry, successor, gateway vetted list
- Custom metadata fields
- Last-writer-wins merge

## Client SDK Pattern

### Initialization

```rust
// 1. Generate or restore mnemonic
let mnemonic = Bip39RootSecretStrategy::random(&mut rng);
let root_secret = Bip39RootSecretStrategy::to_root_secret(&mnemonic);

// 2. Build client with modules
let client = Client::builder(database)
    .with_module(MintClientInit)
    .with_module(WalletClientInit)
    .with_module(LnClientInit)
    .build(root_secret, invite_code)
    .await?;

// 3. Use module extension traits
let operation_id = client.mint().reissue_notes(notes).await?;

// 4. Track operation progress
let status = client.operation_status(operation_id).await?;
```

### Secret Derivation (BIP39)

```
global_root_secret (from BIP39 mnemonic)
  / child_key(0)                    # key-type = per-federation
  / federation_key(federation_id)   # federation-specific
  / child_key(0)                    # wallet number (allows multiple wallets)
  / child_key(0)                    # fedimint-client instance
```

`Bip39RootSecretStrategy` handles: mnemonic generation → `global_root_secret` → derivation path. The client additionally applies federation ID derivation internally to prevent cross-federation reuse.

### OperationId + State Machine Pattern

Operations are long-running, async, state-driven:

```
Operation Start → Generate OperationId (32 random bytes)
      ↓
  Spawn State Machines (one per module involved)
      ↓
  Active States (execute transitions in parallel)
      ↓
  Inactive States (persist in DB for recovery/restart)
      ↓
  Operation Complete (or error)
```

- High-level operations return instantly with `OperationId`
- Progress queried via `client.operation_status(operation_id)`
- State machines persist in database, resume on restart
- `OperationId` groups related API requests for Tor circuit privacy

**State machine examples**:
- Mint: `ReceiveNoteStateMachine` (await blind sig), `ReissueStateMachine` (await validation)
- Wallet: `PegInStateMachine` (await confirmation), `PegOutStateMachine` (await broadcast)
- LN: `OutgoingContractStateMachine` (await preimage)

## Consensus Architecture

### AlephBFT Integration

Federation nodes run three parallel tasks:
1. **API task**: Accept client `Transaction` submissions, return `TransactionStatus`
2. **AlephBFT task**: Gossip proposals, produce identical `ConsensusOutcome` per epoch
3. **Consensus task**: Process outcomes — validate, update DB, perform actions

### Transaction Anatomy

```rust
Transaction {
    inputs: Vec<DynInput>,      // Funds consumed (e-cash spent, peg-in proofs)
    outputs: Vec<DynOutput>,    // Funds created (new notes, peg-outs)
    nonce: u64,                 // Entropy for identical transactions
    signatures: schnorr::Sig,   // MuSig2 multisig of all input keys
}
// Invariant: sum(input amounts) ≥ sum(output amounts) (difference = fees)
```

Inputs and outputs can span modules: e.g., `PegInProof` input (wallet) + `BlindNote` output (mint) = deposit BTC → get e-cash.

### Consensus Flow

```
Client submits Transaction via API
    ↓
API validates, stores as proposal in DB
    ↓
AlephBFT gossips proposals across peers
    ↓
Agreement on ConsensusOutcome (ordered ConsensusItems)
    ↓
Each module processes its items:
  - validate_input() / apply_input()
  - validate_output() / apply_output()
  - consensus_proposal() → submit threshold sig shares etc.
  - begin_consensus_epoch() / end_consensus_epoch()
    ↓
TransactionStatus → success or error
```

Some operations need multiple epochs (e.g., blind signatures require each guardian to contribute a share, then combine).

## Gateway Architecture

The Lightning Gateway (`gatewayd`) bridges Fedimint e-cash and Lightning Network:

- **Multi-federation**: Single gateway can serve multiple federations
- **LN integration**: LND or LDK (internal node) via `fedimint-lightning` abstraction
- **Payment flow (outgoing)**: User locks e-cash in `OutgoingContract` → gateway pays LN invoice → provides preimage → gets e-cash
- **Payment flow (incoming)**: Gateway creates invoice → payer sends LN payment → gateway locks e-cash in `IncomingContract` → user claims

**Ports**: fedimintd P2P 8173, fedimintd API 8174, gateway API 8175

## Database Architecture

### Abstraction Layers

```
DatabaseTransaction (public API)
    → IDatabaseTransaction (commit, subscribe/notify)
    → IDatabaseTransactionOpsCore (raw bytes: insert, get, remove, prefix_scan)
```

**Implementations**: `MemDatabase` (testing), `RocksDbDatabase` (production), `RocksDbReadOnly` (snapshots)

### Key Prefixing & Isolation

```
<GLOBAL_PREFIX><MODULE_ID (2 bytes)><ENTITY_PREFIX>
```

Each module's data is isolated by its `ModuleInstanceId`. Consensus data has no module prefix.

### Transactions

Optimistic locking: concurrent reads, write conflicts detected on commit. All DB operations within transactions.

### Migrations

Per-module `get_database_migrations()`. Tested via snapshots (`FM_PREPARE_DB_MIGRATION_SNAPSHOTS=force`).

## Build System

### Nix-First

- Reproducible dev environment via `nix develop`
- [Cachix binary cache](https://fedimint.cachix.org) — avoid building from scratch
- Trusted user required in `/etc/nix/nix.conf` (or `/etc/nix/nix.custom.conf` for Determinate installer)
- Cross-compilation shell: `nix develop .#cross` (includes WASM target, Android NDK)
- OCI containers: `nix build .#container.fedimintd`
- NixOS module for systemd deployment

### Just Commands

```bash
just build           # cargo build --workspace
just check           # cargo check
just test            # cargo test (builds first)
just clippy          # clippy with warnings-as-errors
just lint            # pre-commit hooks (fmt, semgrep)
just format          # rustfmt + nixfmt (treefmt)
just mprocs          # Launch 4-node dev federation + gateway (visual)
just devimint-env    # Spawn federation in isolation
just final-check     # Pre-PR: lint, format, test, WASM
just test-ci-all     # Full CI test matrix
just docs            # Build & open rustdoc
```

### devimint (Test Orchestrator)

Spawns a complete local environment:
- N federation guardian nodes (fedimintd)
- Bitcoin regtest node (bitcoind)
- Lightning nodes (CLN, LND)
- Gateway instance
- All coordinated via `mprocs` (visual terminal multiplexer)

### WASM Support

- Target: `wasm32-unknown-unknown`
- `fedimint-client-wasm`: JS bindings via wasm-bindgen
- Check: `just check-wasm`
- Core must maintain no-std compatibility

### UniFFI (Mobile)

- `fedimint-client-uniffi`: FFI bindings for iOS/Android
- Enables native mobile wallet apps

## Testing Infrastructure

| Layer | Tool | Purpose |
|-------|------|---------|
| Unit | `cargo test` | Per-crate unit tests |
| Integration | devimint | Full federation + bitcoind + LN nodes |
| DB migration | Snapshots | Known-good state verification |
| WASM | `just check-wasm` | Browser compatibility |
| Upgrade | Version compat tests | Cross-release compatibility |
| Fuzzing | libFuzzer | `fuzz/` crate targets |
| Load | `fedimint-load-test-tool` | Performance benchmarking |

Environment: `FM_*` env vars for test config. Faster timeouts in tests (100ms vs 60s production).

## Code Quality Standards

- **No `unwrap()`**: Always `expect("reason")` with explanation
- **Structured logging**: `field = value` format, multi-line for readability
- **Clippy**: Pedantic + nursery with warnings-as-errors
- **All public items**: Require docstrings
- **Edition**: 2024
- **Formatting**: `treefmt` (rustfmt + nixfmt)
- **CI**: GitHub Actions + Cachix, semgrep, typos checker

## Contribution Workflow

1. Fork + PR to `master` branch
2. CI must pass (clippy, fmt, tests, WASM check)
3. 1 review required for merge
4. `cargo-sort` for dependency ordering
5. Weekly dev calls Mon/Tue/Thu

## Invite Codes

Federation invite codes encode: federation-id + peer endpoints + optional API secret. Base32 format with `fed1` prefix. Used by clients to join a federation.

## CREAM Integration Notes

For CREAM's CURD e-cash, the key integration points are:

1. **fedimint-client** — the SDK we'll use to build wallet functionality
2. **Mint module** — provides e-cash issuance/redemption (the core of CURD tokens)
3. **Wallet module** — BTC ↔ CURD exchange (peg-in/peg-out)
4. **Lightning module** — fast payments between CREAM users via gateway
5. **BIP39 secret derivation** — deterministic wallet recovery
6. **WASM support** — `fedimint-client-wasm` for browser, UniFFI for mobile
7. **State machine pattern** — async operations with `OperationId` tracking

The `fedimint-dummy-*` modules serve as a template for understanding the module pattern. For CREAM, we likely won't create a custom module — we'll use the existing mint module directly via the client SDK.
