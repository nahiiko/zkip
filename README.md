# 🦀 zkip

> Note: zkip is brand new and actively under development. Things will change and bugs may exist. If you find any bugs or have any feature requests, please open an issue.

zkip is a zero-knowledge proof library for proving a user's IP address is **not** from a specified set of countries without revealing the actual IP address. Built with [SP1](https://github.com/succinctlabs/sp1) zkVM.

## Motivation

Many businesses and developers end up storing IP addresses for various reasons:

- Regulatory compliance (tax laws, content licensing, gambling restrictions)
- Proving user location to auditors
- Fraud prevention and geo-restrictions
- Analytics and service customization

But storing IP addresses creates privacy concerns and GDPR obligations. **What if you could prove location without storing the IP?**

(For example: In France, digital service providers must charge VAT on all sales unless they can prove the customer's location. This typically requires storing IP addresses, which is a privacy concern.)

**zkip's solution**: Generate a zero-knowledge proof that verifies "this IP is not from countries X, Y, Z" without revealing or storing the actual IP address. You get verifiable location proofs while keeping user data private.

## How It Works

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              ARCHITECTURE                                    │
└─────────────────────────────────────────────────────────────────────────────┘

┌──────────────┐     POST /prove            ┌──────────────────┐
│              │   {excluded: ["FR","DE"]}  │                  │
│    User      │ ─────────────────────────▶ │    Your API      │
│  (Browser)   │                            │     (zkip)       │
│              │                            │                  │
│  IP: hidden  │                            │ Extracts IP from │
│              │                            │ TCP connection   │
└──────────────┘                            └────────┬─────────┘
       ▲                                             │
       │                                             │ IP + country ranges
       │                                             │ (private inputs)
       │                                             ▼
       │                                    ┌──────────────────┐
       │                                    │                  │
       │   {                                │   SP1 Prover     │
       │     is_excluded: true,             │   (local or      │
       │     proof: "0xabc...",             │    network)      │
       │     timestamp: "..."               │                  │
       │   }                                │ Runs ZK circuit: │
       │                                    │ IP ∉ excluded?   │
       └────────────────────────────────────┴──────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                           WHAT GETS STORED                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│  ✅ Cryptographic proof                                                      │
│  ✅ Boolean result (is_excluded)                                             │
│  ✅ User identifier (e.g., wallet address)                                   │
│  ✅ Timestamp                                                                │
│                                                                              │
│  ❌ IP address (never stored)                                                │
│  ❌ Actual country (never revealed)                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Flow

```mermaid
sequenceDiagram
    participant User as User (wallet: 0x123...)
    participant API as Your API (zkip)
    participant Prover as SP1 Prover

    User->>+API: POST /prove<br/>{excluded: ["FR", "DE"], wallet: "0x123..."}
    Note over API: Extract IP from TCP connection<br/>Load IP to country range table

    API->>+Prover: Private inputs: IP + full range table<br/>Public input: excluded countries
    Note over Prover: ZK Circuit executes:<br/>1. Lookup IP in range table<br/>2. Check if country in excluded list<br/>3. Output: is_excluded = true/false
    Prover-->>-API: proof + is_excluded

    Note over API: Discard IP from memory
    API-->>-User: {proof, is_excluded, timestamp}

    Note over User: Store proof for compliance audit<br/>No IP ever stored
```

## Project Structure

```
zkip/
├── lib/              # Shared code (types and functions used by both program and script)
│   └── src/lib.rs    # IP range types, country check logic
├── program/          # The ZK program (compiles to RISC-V, runs inside SP1 zkVM)
│   └── src/main.rs   # Reads IP, checks ranges, outputs boolean
├── script/           # CLI for testing and generating proofs
│   └── src/bin/
│       ├── main.rs   # Execute or prove the program
│       ├── evm.rs    # Generate EVM-compatible proofs (Groth16/PLONK)
│       └── vkey.rs   # Export verification key for on-chain use
```

### Crate Responsibilities

| Crate       | Purpose                                   | Compilation Target         |
| ----------- | ----------------------------------------- | -------------------------- |
| **lib**     | Shared types and functions (used by both) | Standard Rust (testable)   |
| **program** | ZK circuit that runs inside SP1 zkVM      | RISC-V (via `cargo prove`) |
| **script**  | CLI to trigger execution/proving          | Standard Rust              |

The `lib` crate is optional but recommended. It lets you share types (like `PublicValuesStruct`) between the program and script, and test your logic without compiling to RISC-V.

## Requirements

- [Rust](https://rustup.rs/)
- [SP1](https://docs.succinct.xyz/docs/sp1/getting-started/install)

## Quick Start

### 1. Build the Program

```sh
cd program && cargo prove build
```

Or let it build automatically when running the script.

### 2. Execute (No Proof)

Test the logic without generating a proof:

```sh
cargo run --release -- --execute --ip 8.8.8.8 --exclude FR,US
```

This runs the ZK circuit locally and outputs the result without generating a cryptographic proof.

### 3. Generate a Proof (Local)

```sh
cargo run --release -- --prove --ip 8.8.8.8 --exclude FR
```

Local proving is slow (minutes to hours depending on hardware). For production, use the network.

### 4. Use the Prover Network

The [Succinct Prover Network](https://docs.succinct.xyz/docs/sp1/prover-network/quickstart) provides fast, distributed proof generation.

**Setup:**

1. Generate a key pair: `cast wallet new` (or export from Metamask)
2. Get [PROVE tokens](https://docs.succinct.xyz/docs/sp1/prover-network/quickstart) on Ethereum Mainnet
3. Deposit PROVE at [Succinct Explorer](https://explorer.succinct.xyz/)
4. Configure environment:

```sh
cp .env.example .env
```

```env
SP1_PROVER=network
NETWORK_PRIVATE_KEY=0x...  # Your requester account private key
```

5. Run:

```sh
cargo run --release -- --prove --ip 8.8.8.8 --exclude FR
```

### 5. Generate EVM-Compatible Proof

For on-chain verification (requires network or 16GB+ RAM locally):

```sh
# Groth16 (smaller proof, higher gas to generate)
cargo run --release --bin evm -- --ip 8.8.8.8 --exclude FR --system groth16

# PLONK (larger proof, no trusted setup)
cargo run --release --bin evm -- --ip 8.8.8.8 --exclude FR --system plonk
```

### CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `--ip` | IP address to test | `8.8.8.8` |
| `--exclude` | Comma-separated country codes (ISO 3166-1 alpha-2) | `FR` |
| `--refresh` | Force refresh the GeoIP database | `false` |
| `--execute` | Run without proof (main.rs only) | - |
| `--prove` | Generate proof (main.rs only) | - |
| `--system` | Proof system: `groth16` or `plonk` (evm.rs only) | `groth16` |

### GeoIP Database

The CLI automatically fetches IP-to-country data from [ip-location-db](https://github.com/sapics/ip-location-db) via jsDelivr CDN. The database is cached locally for 30 days. Use `--refresh` to force an update.

## API Design (Future)

```
POST /prove

Request:
{
  "excluded_countries": ["FR", "DE", "IT"],  // Countries to prove absence from
  "user_id": "0x123..."                      // Optional: wallet or identifier
}

Response:
{
  "is_excluded": true,                       // IP not in any excluded country
  "proof": "0xabc...",                       // ZK proof
  "timestamp": "2025-12-18T12:00:00Z"
}
```

## Current Status

🚧 **Proof of Concept**

### Phase 1: ZK Proof Core ✅

- [x] IP range data structure (u32 start/end tuples)
- [x] Multi-country exclusion logic
- [x] ZK circuit implementation (SP1 zkVM)
- [x] CLI with `--ip`, `--exclude`, `--refresh` flags
- [x] Dynamic GeoIP database fetching (30-day cache)
- [x] EVM-compatible proofs (Groth16/PLONK)

### Phase 2 (Future): API Server

- [ ] REST API endpoint
- [ ] IP extraction from HTTP request
- [ ] Proof generation service

### Phase 3 (Future): On-Chain Verification

- [ ] Solidity verifier contract
- [ ] Contract deployment scripts

### Phase 4 (Future): Client Integration

- [ ] Frontend integration
- [ ] Proof storage and retrieval

## Privacy Model

| Party              | Sees IP?                 | Stores IP? |
| ------------------ | ------------------------ | ---------- |
| User               | ✅ (their own)           | N/A        |
| Your Server        | ✅ (transient in memory) | ❌ No      |
| SP1 Prover Network | ✅ (during computation)  | ❌ No      |

**Note**: The SP1 Prover Network is a compute service, not a blockchain. Inputs are processed in memory and discarded after proof generation.

## License

MIT
