# zkip — Improvement & Roadmap Plan

This document is an implementation plan for zkip. It is written so that each task can be
executed independently by an engineer (or AI agent) without additional context. Read the
**Codebase Map** first, then execute tasks in order — earlier tasks unblock or simplify
later ones. Every task lists the files to touch and explicit acceptance criteria.

---

## Codebase Map (current state)

```
zkip/
├── Cargo.toml               # Workspace: lib, program, script
├── lib/src/lib.rs           # Shared: PublicValuesStruct (alloy sol!), is_excluded(), ip_to_u32(), u32_to_ip()
├── program/src/main.rs      # SP1 zkVM guest program: reads inputs, calls is_excluded, commits ABI-encoded public values
├── script/
│   ├── build.rs             # Builds the guest program via sp1-build
│   └── src/bin/
│       ├── main.rs          # CLI `zkip`: --execute / --prove; fetches GeoIP CSV, loads ranges, runs prover
│       ├── evm.rs           # CLI `evm`: Groth16/PLONK proof + JSON fixture (duplicates ~180 lines of main.rs)
│       └── vkey.rs          # Prints verification key bytes32
├── data/
│   ├── countries.csv        # ISO 3166-1: alpha-2 → numeric code mapping (committed)
│   ├── ipv4-country.csv     # GeoIP cache, ~320k rows `start,end,CC` (gitignored, fetched from jsDelivr)
│   └── ipv6-country.csv     # Present locally but UNUSED by any code
├── docs/                    # Vocs documentation site (pages/*.mdx)
└── .github/workflows/
    ├── prove.yml            # workflow_dispatch only; builds + executes program; uses deprecated actions-rs
    └── foundry-test.yml     # workflow_dispatch only; references contracts/ which DOES NOT EXIST yet
```

Key facts an implementer must know:

- Original zkip application code is `AGPL-3.0-only`. The repository was
  bootstrapped from Succinct Labs' MIT-licensed SP1 Project Template and contains
  a CC BY-SA 4.0 country-code dataset; preserve `LICENSE-MIT` and
  `THIRD_PARTY_NOTICES.md`. Dependencies retain their own licenses. Future
  integration surfaces intended for embedding (the JavaScript SDK and Solidity
  verifier) should remain permissively licensed under MIT or Apache-2.0.
- The guest program (`program/`) compiles to RISC-V via `cargo prove build` / `sp1-build`.
  It must stay `no_std`-friendly in practice (no file I/O, no networking, deterministic).
- `zkip-lib` is compiled both for the host and for the zkVM — keep it dependency-light.
- Inputs to the circuit today: **private** = `ip: u32`, `excluded_ranges: Vec<(u32,u32)>`;
  **public (committed)** = `is_excluded: bool`, `timestamp: u32`, `excluded_countries: Vec<u16>`.
- `evm.rs` writes fixtures to `../contracts/src/fixtures` — that directory does not exist
  until Phase 3 creates the contracts package.
- SP1 SDK version is `5.0.8` throughout. Do not upgrade major versions as a side effect
  of another task.

---

## Priority 0 — Soundness fix (architectural, most important)

### Task 0.1: Commit to the GeoIP dataset inside the circuit

**Problem.** The IP range table is a *private, unauthenticated* input. The proof currently
says: "given *some* list of ranges the prover chose, this IP is not in it." A malicious
prover can pass an **empty range list** and produce a valid proof that any IP is
"not in France." The committed `excluded_countries` values are decorative — nothing in
the circuit links them to the ranges that were actually checked. As designed, the proof
has no value to a third-party verifier (auditor, smart contract).

**Fix.** Hash the range data inside the zkVM and commit the digest as a public value.
A verifier then checks the digest equals the published digest of a known dataset version.

**Design principle: bind by construction, verify nothing in-circuit.** The guest does
not need to *validate* the ranges (sortedness, country matching, etc.). It only needs
to hash exactly what it checked the IP against, keyed by the public country list. If
the prover feeds garbage, the proof still succeeds — but the digest won't match the
published reference digest, so the proof is worthless. All "is this the right data?"
logic lives in one place: the digest comparison done by the verifier.

Implementation steps:

1. In `lib/src/lib.rs`, extend the sol! struct:
   ```solidity
   struct PublicValuesStruct {
       bool is_excluded;
       uint32 timestamp;
       uint16[] excluded_countries;
       bytes32 dataset_commitment;   // NEW
   }
   ```
2. Add ONE pure function to `lib/src/lib.rs`, shared verbatim by guest and host:
   ```rust
   /// Digest binding the public country list to the exact ranges checked.
   /// Byte stream: for each country (in given order):
   ///   code u16 LE || range_count u32 LE || (start u32 LE || end u32 LE)*
   pub fn dataset_commitment(countries: &[u16], ranges: &[Vec<(u32, u32)>]) -> [u8; 32]
   ```
   Use the `sha2` crate. Add the SP1 `sha2` precompile patch as a `[patch]` entry in
   the workspace `Cargo.toml` (per SP1 docs) so the guest hashes cheaply; the host
   uses plain `sha2` — same code, no feature gates.
3. Canonical form is produced by the **host**, not enforced by the guest: host sorts
   `excluded_countries` ascending and each country's ranges ascending by start before
   writing them to stdin. Document this two-line rule in the README; it is what a
   dataset publisher follows to compute the reference digest.
4. `program/src/main.rs`: change the private input to `Vec<Vec<(u32, u32)>>` — the
   i-th entry is the ranges for the i-th code in the public `excluded_countries` list
   (positional pairing, so no matching logic exists at all). Guest body stays ~10
   lines: read inputs, `is_excluded` over the flattened ranges,
   `dataset_commitment(...)`, commit.
5. Host prints the digest on every run. Add `--print-commitment` to print it for a
   given `--exclude` list without proving (what a publisher runs per dataset release).

**Acceptance criteria:**
- `cargo run --release -- --execute --ip 8.8.8.8 --exclude FR` prints a 32-byte
  commitment, and guest-committed digest == host-computed digest.
- Unit tests on `dataset_commitment`: fixed known-answer vector; any tamper (drop a
  range, reorder countries, swap a range between countries) changes the digest.
- README "How It Works" and `docs/pages/architecture/*.mdx` updated to describe the
  commitment and the verifier's obligation to check it.

**Trust model note to document:** `timestamp` remains prover-supplied and unverified;
a verifier must treat it as a claim, not a fact (on-chain verification can compare it
to `block.timestamp` within a tolerance). State this explicitly in the docs.

---

## Priority 1 — Technical debt (do before adding features)

### Task 1.1: Deduplicate `main.rs` and `evm.rs`

`script/src/bin/evm.rs` copies ~180 lines verbatim from `main.rs`: `get_cache_path`,
`is_cache_stale`, `fetch_geoip_database`, `ensure_geoip_database`, `load_country_codes`,
`parse_excluded_countries`, `load_ip_ranges_for_countries`, the `GEOIP_URL` /
`CACHE_MAX_AGE_DAYS` constants, and the stdin-assembly block.

Steps:
1. Create `script/src/lib.rs` (add `[lib] name = "zkip_script"` implicitly by having the
   file; no Cargo.toml change needed beyond it being picked up automatically — verify).
2. Move all functions above into it (suggested module: `zkip_script::geoip` and
   `zkip_script::inputs`). Add a `pub fn build_stdin(ip, per_country_ranges, excluded_countries, timestamp) -> SP1Stdin`
   helper so both binaries assemble inputs identically (input ordering bugs between the
   two binaries are otherwise easy to introduce).
3. Rewrite both binaries to import from the lib. `evm.rs` must also gain
   `dotenv::dotenv().ok();` (currently only `main.rs` loads `.env`, so network proving
   config silently doesn't apply to the EVM binary).

**Acceptance:** no function bodies duplicated between the two binaries; both compile;
`cargo run --release -- --execute` behaves as before.

### Task 1.2: Add unit tests (currently zero tests in the repo)

All in `lib/src/lib.rs` (`#[cfg(test)] mod tests`) plus a new `script` test file:
- `is_excluded`: empty ranges → true; IP below/above/inside/on-boundary of a range;
  multiple ranges.
- `dataset_commitment` (after Task 0.1): one fixed known-answer vector; any tamper
  (drop a range, reorder countries, swap a range between countries) → different digest.
- In `script/tests/`: `parse_excluded_countries` happy path, unknown code error,
  empty input error, whitespace/lowercase normalization; `load_ip_ranges_for_countries`
  against a small inline temp CSV; invalid `--ip` strings rejected (via `Ipv4Addr`
  parse, per Task 1.3).

**Acceptance:** `cargo test --workspace` passes (note: guest program is excluded from
host tests automatically since it's `no_main`; if `cargo test -p zkip-program` breaks
the workspace test run, test with `cargo test -p zkip-lib -p zkip-script`).

### Task 1.3: Small API cleanups in `lib`

1. Change `is_excluded(ip: u32, excluded_ranges: Vec<(u32, u32)>)` to take
   `&[(u32, u32)]` — it's called with clones today (`main.rs:247` clones the whole
   range vector just for the assert).
2. **Delete** `ip_to_u32` and `u32_to_ip` entirely. The standard library already does
   this: `u32::from(ip_str.parse::<Ipv4Addr>()?)` to parse, `Ipv4Addr::from(ip)` to
   display. Update the two call sites in `script/` to use `Ipv4Addr` directly
   (`core::net::Ipv4Addr` is stable, so this works in the guest too if ever needed).
   Less code to own beats preserving a redundant API in a 0.1.0 crate.
3. Update all call sites in `program/` and `script/`.

**Acceptance:** workspace compiles; tests from Task 1.2 still pass.

### Task 1.4: Real CI

Current workflows are `workflow_dispatch`-only (never run automatically), use the
deprecated/archived `actions-rs/toolchain` action, and `foundry-test.yml` targets a
nonexistent `contracts/` directory.

1. Add `.github/workflows/ci.yml` running on `push` to `main` and on `pull_request`:
   - `dtolnay/rust-toolchain@stable`
   - `cargo fmt --all -- --check`
   - `cargo clippy -p zkip-lib -p zkip-script -- -D warnings`
     (clippy on the guest program requires the SP1 toolchain; skip it in this job)
   - `cargo test -p zkip-lib -p zkip-script`
   - Cache with `Swatinem/rust-cache@v2`.
   Note: building `zkip-script` triggers `build.rs` → guest build; either install the
   SP1 toolchain in CI (see `prove.yml` steps) or set the sp1-build "skip build" env var
   (`SP1_SKIP_PROGRAM_BUILD=true`) for lint/test jobs if a prebuilt ELF is not needed —
   verify which the sp1-build version supports and pick one; document the choice in the
   workflow file.
2. In `prove.yml`, replace `actions-rs/toolchain` with `dtolnay/rust-toolchain@1.85.0`,
   keep it `workflow_dispatch` (it's expensive), and add a weekly `schedule:` trigger so
   dataset/toolchain drift is caught.
3. Leave `foundry-test.yml` as `workflow_dispatch`-only until Phase 3 creates
   `contracts/`; add a comment in the file saying so.

**Acceptance:** CI workflow green on a test branch; fmt/clippy/test all enforced.

### Task 1.5: Pin and verify the GeoIP dataset

Reproducibility problem: `GEOIP_URL` points at the mutable `latest` npm tag on jsDelivr,
so two runs on different days can silently use different data (and thus different
commitments after Task 0.1).

1. Pin a version in the URL:
   `https://cdn.jsdelivr.net/npm/@ip-location-db/geo-whois-asn-country@<X.Y.Z>/geo-whois-asn-country-ipv4-num.csv`
   (look up the current version on npm at implementation time).
2. Store the expected SHA-256 of that file next to the constant; verify after download,
   fail with a clear error on mismatch.
3. Add `--geoip-url` and `--geoip-sha256` CLI overrides for users who want newer data.
4. Persist the pinned version in the cache filename (`ipv4-country-<X.Y.Z>.csv`) so a
   version bump invalidates the cache naturally; keep the 30-day staleness check only
   as a warning (a pinned file doesn't go stale, but the *pin* does — print a hint to
   check for a newer dataset release).
5. Update README "GeoIP Database" section.

**Acceptance:** fresh clone downloads the pinned file, checksum verified; corrupting the
cached file and re-running with `--refresh` recovers; wrong checksum → hard error.

### Task 1.6: Misc hygiene (batch into one small PR)

- Remove `.DS_Store` files from git and add `.DS_Store` (already covered) — run
  `git rm --cached $(git ls-files | grep DS_Store)`.
- `rust-toolchain` pins `channel = "stable"` (a moving target) while `prove.yml` pins
  `1.85.0` — pin the same concrete version in `rust-toolchain` for reproducibility.
- Document the Y2106 limitation of `uint32 timestamp` in a code comment on the struct
  (do not change the type now; changing public values ABI is coupled to Task 0.1 —
  if Task 0.1 lands first, consider `uint64` in the same ABI change).
- `data/ipv6-country.csv` is unused: either delete it or reference it in Task 4.2
  (IPv6). It's gitignored, so nothing to remove from git; just note it.

---

## Priority 2 — Phase 2 from README: REST API server

New crate `api/` added to the workspace. Framework: **axum** (tokio ecosystem, plays
well with `sp1-sdk`'s async network prover). Reuse `zkip-script`'s lib from Task 1.1 —
if the geoip/inputs modules are needed by both `script` and `api`, promote them into a
new `host-lib/` crate (name: `zkip-host`) instead of depending on the script crate.

### Task 2.1: Service skeleton

- `POST /prove` — request `{ "excluded_countries": ["FR","DE"], "user_id": "0x..." }`
  (per README "API Design (Future)"). `user_id` optional, opaque, max 128 chars.
- Extract client IP: from the socket peer address by default; trust
  `X-Forwarded-For` **only** when `ZKIP_TRUSTED_PROXY=true` env var is set (take the
  first address in the header, validate it parses as IPv4; reject IPv6 with a 400 and
  a clear "IPv4 only for now" message).
- Proving takes minutes → **must be async jobs**, not a blocking handler:
  - `POST /prove` → validates, loads ranges, builds stdin, enqueues, returns
    `202 { "job_id": "<uuid>" }`. The raw IP must not be stored in the job record —
    only the assembled `SP1Stdin` lives in memory until the prover consumes it.
  - `GET /proofs/{job_id}` → `{ "status": "pending" | "complete" | "failed", ... }`;
    when complete: `{ is_excluded, proof: "0x...", public_values: "0x...", vkey, timestamp, dataset_commitment }`.
  - In-memory job store (`tokio::sync::RwLock<HashMap<Uuid, Job>>`) is fine for v1;
    completed jobs evicted after a TTL (env `ZKIP_JOB_TTL_SECS`, default 3600).
- Load the GeoIP dataset **once at startup** into memory (indexed by country), not per
  request.
- Config via env: `ZKIP_BIND_ADDR` (default `0.0.0.0:8080`), `SP1_PROVER`,
  `NETWORK_PRIVATE_KEY` (never log it), `ZKIP_TRUSTED_PROXY`, `ZKIP_JOB_TTL_SECS`.
  Update `.env.example` (names only, no values).
- Basic hardening, minimal version: axum's default body limit, and a **bounded job
  queue** (`tokio::sync::mpsc::channel(N)`) returning 429 when full — that is the
  rate limit for v1; do not add a rate-limiter dependency. `tracing` logs must
  **never include the client IP** (this is the entire point of the project — grep
  the crate for the ip variable name in a review pass).

**Acceptance:** with `SP1_PROVER=mock` (SP1's mock prover), an end-to-end
`curl POST /prove` → poll `GET /proofs/{id}` returns a completed mock proof; unit tests
for IP extraction (proxy trusted/untrusted), validation errors, and job TTL eviction;
README + `docs/pages/coming-soon/rest-api.mdx` rewritten as real docs (move out of
`coming-soon/`, update `vocs.config.ts` sidebar).

### Task 2.2: Proving backends

- Wire `ProverClient::from_env()` so mock / cpu / network all work via `SP1_PROVER`.
- The prover `setup()` is expensive: do it once at startup, share `(pk, vk)` via
  `Arc`.
- Expose `GET /vkey` returning the verification key bytes32 (same output as the
  `vkey` binary) and `GET /commitment?exclude=FR,DE` returning the dataset commitment,
  so verifiers can fetch both reference values.

---

## Priority 3 — Phase 3 from README: On-chain verification

### Task 3.1: Contracts package

Create `contracts/` as a Foundry project (this is what `foundry-test.yml` and the
`evm.rs` fixture path already expect). Mirror the SP1 project template
(`sp1-project-template` on GitHub, `contracts/` directory) rather than inventing a
layout:

1. `forge init contracts --no-git`, add `sp1-contracts` dependency
   (`succinctlabs/sp1-contracts`) for `ISP1Verifier`.
2. `contracts/src/Zkip.sol`:
   ```solidity
   contract Zkip {
       address public immutable verifier;      // SP1 verifier gateway
       bytes32 public immutable zkipProgramVKey;
       bytes32 public immutable expectedDatasetCommitment;

       function verifyZkipProof(bytes calldata publicValues, bytes calldata proofBytes)
           external view
           returns (bool isExcluded, uint32 timestamp, uint16[] memory excludedCountries)
       {
           ISP1Verifier(verifier).verifyProof(zkipProgramVKey, publicValues, proofBytes);
           (isExcluded, timestamp, excludedCountries, bytes32 commitment) =
               abi.decode(publicValues, (bool, uint32, uint16[], bytes32));
           require(commitment == expectedDatasetCommitment, "unknown dataset");
       }
   }
   ```
   (Adjust decode tuple to the final ABI from Task 0.1. The dataset check is what makes
   Task 0.1 meaningful on-chain.)
3. Foundry tests reading `contracts/src/fixtures/groth16-fixture.json` /
   `plonk-fixture.json` (produced by `cargo run --bin evm`), asserting decode + a
   mock-verifier path; use SP1's `SP1MockVerifier` for unit tests.
4. Deploy script (`forge script`) parameterized by verifier gateway address, vkey,
   and commitment; document the canonical SP1 verifier gateway addresses per chain
   (link to SP1 docs rather than hardcoding).
5. Enable `foundry-test.yml` on `pull_request` (paths filter: `contracts/**`).

**Acceptance:** `forge test` green in CI with committed fixtures; docs page
`docs/pages/coming-soon/on-chain.mdx` promoted to a real guide.

---

## Priority 4 — Phase 4: Commercial adoption package

**Who buys this.** Teams that already run IP geofencing for compliance and currently
store IPs to prove they did it:

1. **Web3 / crypto** — front-ends, token sales, and airdrops that must block
   sanctioned or restricted jurisdictions (OFAC lists, "not available to US persons").
   Today they log IPs behind a Cloudflare/MaxMind check. Highest urgency, most
   ZK-native, and the only segment that needs on-chain verification (Phase 3).
2. **EU digital-service sellers** — must evidence customer location for VAT
   (the README's France example). Today they retain IPs under GDPR pain.
3. **iGaming / gambling operators** — licensing requires geo-restriction evidence
   for regulators.

All three already do the check; zkip's pitch is *replace "store the IP" with "store
the proof."* "No questions asked" adoption therefore means: they never pick country
lists, never learn ZK, never build infrastructure — one dependency, one deploy
command, one published trust file. Every task below serves that.

Execute in this order — 4.1 is a correctness prerequisite for a compliance product,
and 4.3 is the artifact every other task references.

### Task 4.1: IPv6 support (adoption prerequisite, not a nice-to-have)

A compliance product that silently misses mobile/IPv6 users is not sellable — a
customer's first auditor question would be "what about IPv6?". Do this before any
go-to-market task.

- Upstream dataset exists (`...-ipv6-num.csv`; `data/ipv6-country.csv` is already
  gitignored). Represent addresses as `u128`; generalize `is_excluded` and
  `dataset_commitment` over `u128` (simplest: always use `u128` in the circuit and
  map IPv4 in — one code path, one vkey, no v4/v6 branching).
- This changes guest inputs → new vkey and commitment format. It is the designated
  ABI-breaking change: bundle the `uint64 timestamp` fix (Task 1.6) into it and add a
  `uint16 circuit_version` field to `PublicValuesStruct` so future breaks are cheap.
- API: accept both address families transparently; SDK/customers never see this.

### Task 4.2: Named exclusion presets

Customers must not curate country lists themselves — that's a legal judgment they
don't want. Ship maintained presets:

- One file, `presets/presets.json`: `{ "preset_id": { "description": ...,
  "countries": ["IR","KP",...], "source": "<link to legal basis>", "updated": ... } }`.
  Initial presets: `ofac-comprehensive` (comprehensively sanctioned jurisdictions),
  `us-persons` (US + territories), `eu` (member states). Keep the list small and
  sourced; a wrong preset is a liability.
- CLI `--exclude-preset ofac-comprehensive` and API
  `{ "preset": "ofac-comprehensive" }` (mutually exclusive with `excluded_countries`).
- Each preset's dataset commitment is published in the trust bundle (Task 4.3), so
  "verify a proof used the OFAC preset" is a single bytes32 comparison.

**Acceptance:** proving with a preset works end-to-end; preset file changes trigger a
CI job that recomputes and updates published commitments.

### Task 4.3: Trust bundle — the single adoption artifact

One versioned JSON answering every "can I trust this proof?" question:

```json
{
  "bundle_version": "1",
  "circuit_version": 1,
  "vkey": "0x...",
  "dataset": { "name": "geo-whois-asn-country", "version": "X.Y.Z", "sha256": "..." },
  "commitments": { "ofac-comprehensive": "0x...", "us-persons": "0x...", "eu": "0x..." },
  "verifiers": { "1": "0x...", "8453": "0x..." }
}
```

- Lives at `trust/trust.json` in the repo (git history = audit trail), served by the
  API at `GET /trust.json`, and rendered as a docs page.
- Regenerated by one script (`cargo run --bin trust-bundle`) that runs vkey +
  commitment computation; CI fails if the committed file is stale.
- Deploy `Zkip.sol` (Phase 3) to one mainnet chain and one L2 the target segment
  actually uses (suggest Ethereum mainnet + Base; decide at implementation time),
  record addresses here.

This file is what a customer pins in their code and what their auditor reads. It
replaces all per-customer trust questions.

### Task 4.4: Drop-in TypeScript SDK (`@zkip/client` on npm)

- New `sdk/ts/` package. Zero runtime dependencies (fetch-based). Surface:
  `const zkip = createClient({ baseUrl }); const proof = await zkip.prove({ preset: "ofac-comprehensive", userId })`
  — handles POST + polling internally, returns a typed
  `{ isExcluded, proof, publicValues, timestamp, datasetCommitment }`.
- `zkip.verifyAgainstTrustBundle(proof)` — fetches `/trust.json`, checks commitment
  and vkey match. Optional `viem` peer dependency for on-chain submission helpers.
- The five-line quickstart in the README of the package IS the product demo. If the
  quickstart needs more than five lines, simplify the SDK, not the quickstart.

**Acceptance:** example script against a local mock-prover API passes in CI;
published to npm on git tag; semver + changelog from day one.

### Task 4.5: One-command deployment

- Multi-stage `Dockerfile` for the API (build with SP1 toolchain, run on slim base),
  published to GHCR on release: `docker run -p 8080:8080 ghcr.io/<org>/zkip` starts
  in mock-prover mode and serves a working `/prove` immediately — a prospect gets a
  proof in their terminal within two minutes of finding the repo.
- Production = same image + three env vars (`SP1_PROVER=network`,
  `NETWORK_PRIVATE_KEY`, `ZKIP_TRUSTED_PROXY`); one `docker-compose.yml` example.
- `GET /health` endpoint (add in Task 2.1 if not present).

### Task 4.6: Segment-targeted integration recipes + compliance docs

Docs restructured so each buyer lands on a page written for *their* problem, each with
copy-paste code reaching a stored proof in under an hour:

- `docs/pages/solutions/web3-geofencing.mdx` — Next.js middleware gating a dApp
  front-end + Solidity snippet gating an airdrop claim with the on-chain verifier.
- `docs/pages/solutions/vat-compliance.mdx` — Express/Next.js checkout middleware,
  what to store per transaction (proof, public values, user id, timestamp), sample
  audit-log schema.
- `docs/pages/solutions/igaming.mdx` — same pattern, licensing framing.
- One auditor-facing page: what a zkip proof attests, exactly what to check
  (vkey → trust bundle, commitment → preset, timestamp tolerance), and template
  GDPR/data-retention language ("we retain cryptographic proofs of location
  compliance; IP addresses are processed transiently and never stored").
  Mark it as a template requiring the customer's own legal review — do not present
  it as legal advice.

**Acceptance:** every recipe is a runnable snippet tested against the Docker image in
CI (a script that boots the container and runs each snippet's happy path).

### Task 4.7: Circuit efficiency (only if cycle counts become a problem)

Current guest does a linear scan over the excluded countries' ranges (thousands to tens
of thousands of ranges — fine). After Task 0.1, hashing dominates. If proving cost or
latency blocks a customer: precompute the dataset Merkle tree host-side, prove
non-membership with sorted-adjacent-leaves + Merkle paths against a published root
instead of hashing the full per-country table in-circuit. Do **not** do this
preemptively; measure with `--execute` cycle counts first and record numbers in the PR.

---

## Priority 5 — Go-to-market: finding these customers and selling to them

GTM runs **in parallel** with Phase 4 once Task 4.5 (Docker two-minute demo) exists —
never before, because every motion below ends with "try it now" and that link must
work flawlessly. Sequencing across segments: **web3 only, until it produces revenue.**
VAT sellers are motion #2 (partner-led), iGaming is #3 (conference/partner-led, long
cycle) — do not split attention across all three at once.

Tasks below are executable by an agent except where marked **[founder]** — those are
decisions or human relationships that must not be delegated.

### Task 5.1: Positioning, pricing, and the two-minute proof

- One-liner per segment, written from the buyer's pain, not the tech:
  - Web3: *"Geo-block your launch without keeping an IP database that contradicts
    your privacy policy."*
  - VAT: *"VAT location evidence without storing IPs under GDPR."*
  - iGaming: *"Regulator-grade geo-restriction evidence, zero PII retained."*
  "Zero-knowledge" appears in the subhead, never the headline — the buyer is buying
  liability reduction, not cryptography.
- **[founder]** Licensing and pricing decision. The prover, API server, CLI, guest,
  and shared core use `AGPL-3.0-only`; the JavaScript SDK and Solidity verifier stay
  MIT or Apache-2.0 so proprietary clients can integrate over stable boundaries.
  Preserve the Succinct template's MIT notice and all dataset/dependency notices.
  Self-hosting remains the trust story. Add a hosted API tier priced
  **per proof** with a monthly floor — proving has real marginal cost (Succinct
  network fees), so per-proof pricing is both honest and easy to justify. Publish the
  price; "contact us" pricing kills the no-questions-asked motion.
- Landing update on the docs site: segment one-liners above the fold, the Docker
  one-liner, the 5-line SDK quickstart, and a link to `trust.json`. Nothing else.

**Acceptance:** a stranger from each segment can state what zkip does for *them*
after 10 seconds on the page; docker → first proof genuinely under two minutes,
timed and re-verified in CI (Task 4.5's script).

### Task 5.2: Ecosystem distribution (web3 inbound engine)

zkip is built on SP1 — Succinct's ecosystem is free, warm distribution. Work the
list:

- Submit to Succinct's built-with-SP1 showcase / ecosystem page; apply to their
  grants program if open (funding + endorsement + their audience in one move).
- PRs adding zkip to `awesome-zk`, `awesome-sp1`, and ZK-tooling lists.
- One deep technical writeup — "Proving where a user *isn't* from, without seeing
  where they are" — covering the dataset-commitment design (Task 0.1) honestly,
  including the trust model's limits. Publish on the docs site, post to HN, X,
  and submit to ZK newsletters (Zero Knowledge podcast's newsletter, Week in
  Ethereum News, The Defiant). Technical honesty IS the marketing in this audience.
- **[founder]** ETHGlobal / ZK hackathon presence: sponsor a small bounty
  ("best use of zkip") or just demo at one event. Hackathons are where the engineers
  who pick geofencing tooling for the next token launch actually are.

**Acceptance (measurable):** showcase submission sent; ≥3 list PRs merged; writeup
published with ≥1 external pickup; track GitHub stars / Docker pulls / npm installs
weekly as the inbound baseline.

### Task 5.3: Trigger-based outbound (web3)

The beauty of this segment: **prospects publicly announce the exact moment they have
the problem.** Every token launch, airdrop, or points program that publishes
"not available to residents of the US / restricted jurisdictions" is running a geo
check and logging IPs to prove it — that announcement is the trigger.

- Build the watchlist (weekly, can be an agent-run routine): airdrop trackers and
  launch calendars (CoinGecko/CoinMarketCap airdrop pages, launchpad announcement
  feeds, X searches for "restricted jurisdictions" / "not available to US persons"
  in launch threads). Output: ~10 qualified leads/week into a simple spreadsheet CRM
  (name, project, launch date, geo-restriction quote, contact, status). No CRM
  software until the spreadsheet hurts.
- Contact the **founder or engineering lead**, not a compliance person — in web3 the
  engineer picks the tool. Channel: whichever is public (X DM, Telegram, email) —
  this audience answers DMs.
- Outreach template (short, specific, zero marketing language):
  > Saw [launch] excludes [jurisdictions]. You're presumably logging IPs to evidence
  > that, which contradicts a privacy-first launch. zkip stores a ZK proof of
  > "IP ∉ excluded countries" instead of the IP — 5-line integration, open source,
  > `docker run` demo in 2 minutes: [link]. Built on SP1. Want the auditor one-pager?
- Timing rule: reach out **before** the launch date (they're deciding tooling) or
  right after a launch gets criticized for geo-handling. Never generic-blast.
- **[founder]** The actual conversations and closes. Target: 10 outreaches/week,
  every reply answered same day, pipeline reviewed weekly.

**Acceptance:** watchlist routine producing ≥10 leads/week; template A/B'd after the
first 30 sends (track reply rate, iterate the first sentence only); first 3 design
partners get white-glove integration free in exchange for a named case study.

### Task 5.4: Leveraged channels (one deal = many customers)

Direct sales is the bootstrap; platforms are the business:

- **Launchpads & token-sale platforms** (CoinList, Fjord Foundry, Legion, and the
  long tail): they run geo-restriction for *every* launch they host. One platform
  integration = every launch on it becomes a zkip user. This is the single
  highest-leverage deal in the plan — **[founder]** owns these conversations;
  agents prepare per-platform integration briefs (how their launch flow works,
  where zkip slots in, estimated integration effort).
- **VAT segment via partners, not direct:** sellers using merchant-of-record
  (Paddle, Lemon Squeezy) have no VAT problem — they are NOT leads. Leads are
  Stripe-direct sellers handling VAT themselves. Reach them through VAT compliance
  tools (Quaderno, Taxually, hellotax) via co-marketing/integration, and through
  indie-SaaS communities (MicroConf, Indie Hackers) with the VAT recipe from
  Task 4.6 as the content hook.
- **iGaming:** the incumbent is GeoComply; do not attack head-on. Position as the
  privacy/evidence layer, enter via platform providers and one industry conference
  (SBC Summit or ICE) **only after** web3 revenue exists. Until then, this segment
  gets nothing but its docs page.

**Acceptance:** integration brief written for top 3 launchpads; one VAT-tool
co-marketing conversation opened; iGaming explicitly deferred in writing (this line).

### Task 5.5: Sales collateral that closes compliance buyers

The engineer says yes in the quickstart; the sale still dies with their lawyer
unless you arm the engineer:

- **Auditor one-pager** (exists as a docs page per Task 4.6; also render as PDF):
  what a proof attests, what to verify, what is NOT proven (VPN caveat, timestamp
  trust — stated plainly; hiding limits loses compliance buyers permanently).
- **Legal memo template** the customer hands their counsel: how zkip maps to their
  geo-restriction obligation, GDPR data-minimization framing, marked "template —
  requires your own legal review."
- **Security FAQ**: trust model, what the prover network sees, self-host option,
  link to `trust.json`. One page. Add "commission an external audit of the circuit"
  as a **[founder]** budget line — for compliance customers an audit is a sales
  asset, not a cost.

**Acceptance:** all three documents linked from every solutions page; a design
partner's lawyer has actually read the memo template and their feedback is folded in.

### Task 5.6: Funnel metrics and cadence

- North-star metric: **proofs generated per week** (mock proofs on the demo count
  separately as top-of-funnel). Instrument: Docker pulls, npm installs, `trust.json`
  fetches, hosted-API signups, proofs/week.
- Weekly rhythm (small enough to actually happen): refresh watchlist → 10 outreaches
  → answer everything → update spreadsheet → one metric review. No dashboards until
  the spreadsheet hurts.

---

**The honest GTM summary:** find web3 teams at the moment they publicly announce a
geo-restricted launch, show them a two-minute demo that replaces their IP logs with
a proof, close the first three as free named design partners, then convert the
motion into launchpad partnerships where one integration ships zkip to every launch
on the platform. VAT and iGaming wait their turn.

---

## Suggested execution order

| Order | Task | Size | Depends on |
|-------|------|------|-----------|
| 1 | 1.1 dedupe script code | S | — |
| 2 | 1.2 + 1.3 tests & lib cleanup | S | 1.1 |
| 3 | 1.4 CI | S | 1.2 |
| 4 | 1.5 pin dataset | S | 1.1 |
| 5 | 0.1 dataset commitment | M | 1.1, 1.5 |
| 6 | 1.6 hygiene | XS | — |
| 7 | 2.1 + 2.2 REST API | L | 0.1 |
| 8 | 3.1 contracts | M | 0.1 |
| 9 | 4.1 IPv6 (+ ABI break: timestamp, circuit_version) | M | 0.1 |
| 10 | 4.2 presets | S | 0.1 |
| 11 | 4.3 trust bundle + deployed verifiers | M | 3.1, 4.1, 4.2 |
| 12 | 4.4 TypeScript SDK | M | 2.1, 4.3 |
| 13 | 4.5 Docker one-command deploy | S | 2.1 |
| 14 | 4.6 recipes + compliance docs | M | 4.3–4.5 |
| 15 | 5.1–5.6 go-to-market | ongoing | starts when 4.5 lands; runs parallel to 4.6 |
| 16 | 4.7 circuit efficiency | — | only if measured need |

Rules for implementers:
- **Simplest solution that meets the acceptance criteria wins.** No new dependency,
  abstraction, trait, or config option unless a task explicitly asks for it. When the
  standard library or an existing crate in the tree can do it, use that. If your diff
  feels large for the task size, it is — stop and reduce.
- One task per PR. Update README and the relevant `docs/pages/*.mdx` in the same PR as
  the behavior change (repo rule: docs updated alongside features).
- Never read or modify `.env`; use `.env.example` for documenting variables.
- Any change to guest inputs or `PublicValuesStruct` changes the vkey — regenerate with
  `cargo run --bin vkey` and update fixtures/docs in the same PR.
- Small commits, no Co-Authored-By lines.
