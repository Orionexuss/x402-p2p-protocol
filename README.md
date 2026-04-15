# x402 Protocol

x402 is a research-first, payment-aware P2P protocol inspired by BitTorrent.
It keeps torrent metadata compatibility, but changes peer behavior from "free transfer" to "cryptographically gated transfer with on-chain settlement".

## TL;DR

- `x402` is a Rust workspace with 4 main parts:
  - `cli`: user entrypoint (`create`, `inspect`, `serve`, `download`, `tracker`)
  - `x402-core`: torrent parsing/building + custom peer protocol
  - `tracker`: Axum HTTP tracker for announce/discovery/report/stats
  - `x402-contract`: Anchor Solana escrow program
- Torrent files remain standard BitTorrent style (`announce` + `info` dictionary).
- The peer protocol adds payment/auth messages on top of a BitTorrent-like flow.
- Seeder advertises priced swarms, leecher locks payment on-chain, data flows piece-by-piece with secret/key reveals, and seeder claims funds by proved secrets.
- This repo is intentionally experimental and educational, not production-grade.

## Project Goals

x402 explores one central question:

> Can we preserve BitTorrent-like distribution efficiency while enforcing economic fairness at protocol level?

Core properties:

- Content remains info-hash-addressed.
- Piece-level transfer remains chunked and verifiable.
- Incentive enforcement moves from reputation-only to cryptographic + on-chain checks.

## Workspace Layout

```text
x402-p2p-protocol/
  Cargo.toml                    # Workspace root
  cli/                          # CLI binary crate
  x402-core/                    # Protocol + torrent + networking library
  tracker/                      # HTTP tracker server (Axum)
  x402-contract/                # Anchor program workspace
  torrents/                     # Seeder-side .torrent metadata files
  data/                         # Seeder payload files keyed by infohash
  seeder.json                   # Price map: info hash -> decimal USDC string
```

Workspace members are:

- `cli`
- `x402-core`
- `tracker`
- `x402-contract/programs/*`

## Architecture

```text
CLI (create/serve/download)
        |
        v
x402-core peer protocol + torrent metadata
        |
        +--> tracker HTTP API (announce/discover/report)
        |
        +--> Solana escrow program (lock + claim/refund)
```

### Component Responsibilities

`cli`:

- Creates `.torrent` files or magnet URIs.
- Inspects torrent/magnet metadata.
- Runs seeder mode and leecher mode.
- Can launch tracker process via subcommand.

`x402-core`:

- Torrent builder/parser.
- Magnet parsing.
- Custom wire protocol (`AuthProof`, `LockedPayment`, `PieceExchange`, `Extended`).
- Seeder/leecher engines.
- Tracker HTTP client.

`tracker`:

- In-memory swarm registry.
- `announce` and `discover` endpoints.
- Reputation penalties via reports.
- Health/stats and stale peer cleanup.

`x402-contract` (Anchor):

- Escrow lock with merkle commitment.
- Claim by secret proofs.
- Release/refund paths.

## Protocol Overview

x402 uses a 7-stage session timeline in both seeder and leecher logs.

1. Handshake
2. Extended metadata negotiation (optional when downloading from magnet)
3. Leecher auth proof
4. Seeder auth acknowledgment
5. Leecher locked-payment commitment
6. Seeder payment acknowledgment
7. Piece exchange

### Wire Message IDs

- `0`: `AuthProof`
- `1`: `AuthOk`
- `2`: `LockedPayment`
- `3`: `PaymentAck`
- `4`: `PieceExchange`
- `20`: `Extended`

### Handshake

x402 handshake extends classic BitTorrent handshake by including:

- `info_hash` (20 bytes)
- `peer_id` (20 bytes)
- `price` (`u64`, big-endian, minor units)
- extension support bit for BEP 10 metadata flow

### Magnet Metadata Path

When leecher source is magnet:

- extended handshake negotiates `ut_metadata`
- metadata pieces are fetched in pipeline
- metadata is reassembled and SHA1-checked against info hash

### Payment Path

- Leecher generates per-piece secrets and Merkle root.
- Leecher submits `lock_payment` on-chain with root and expected piece count.
- Leecher sends `LockedPayment` message containing root.
- Seeder verifies escrow/root/amount/secret-count against devnet account data.

### Piece Exchange Path

- Leecher requests piece `i` with hash commitment and proof.
- For `i > 0`, leecher reveals secret of `i - 1`.
- Seeder validates Merkle proof + revealed secret before revealing previous key and sending next ciphertext.
- Final secret triggers plain delivery of final piece.
- Seeder settles on-chain via `claim_by_secrets` with collected secret proofs.

## Torrent Compatibility

x402 keeps the torrent metadata model compatible for single-file torrent use in this repo.

Expected structure:

```text
{
  "announce": "<tracker-url>",
  "info": {
    "name": "<filename>",
    "piece length": <integer>,
    "pieces": <20-byte SHA1 concatenation>,
    "length": <file-size>
  }
}
```

Info hash rule:

```text
SHA1(bencode(info_dictionary))
```

## Data Model and Local Files

Seeder loads swarms from local files:

- `./torrents/*.torrent`
- `./data/<infohash>` or `./data/<infohash>.<extension>`

Payload resolution behavior:

- exact filename match wins
- otherwise a single `<infohash>.<ext>` match is accepted
- no match or multiple matches for same hash is treated as invalid and skipped

### `seeder.json` price configuration

Format:

```json
{
  "info_hashes": [
    {"<40-char-hex-infohash>": "5.50"},
    {"<40-char-hex-infohash>": "10.00"}
  ]
}
```

Notes:

- one mapping per array item
- price must be a JSON string, not number
- parsed to USDC minor units (1e6 precision)

## CLI Reference

Binary name: `x402` (crate: `cli`)

### `create`

Create torrent metadata or a magnet URI.

```bash
cargo run -p cli -- create <file> \
  --tracker http://localhost:6969/announce \
  --output <name>.torrent
```

Flags:

- `--tracker`, `-t`: announce URL (default `http://localhost:6969/announce`)
- `--output`, `-o`: output file path
- `--magnet`, `-m`: emit magnet URI instead of writing `.torrent`

### `inspect`

Inspect magnet URI or torrent file.

```bash
cargo run -p cli -- inspect ./sample.torrent
cargo run -p cli -- inspect 'magnet:?xt=urn:btih:...'
```

### `serve`

Run seeder TCP listener and announce available torrents.

```bash
cargo run -p cli -- serve \
  --listen 0.0.0.0:6881 \
  --tracker http://localhost:6969
```

Behavior:

- loads local torrents/payloads
- reads `seeder.json` prices
- announces each seeded info hash to tracker
- listens for leecher sessions and serves piece protocol

### `download`

Download from magnet or torrent source.

```bash
cargo run -p cli -- download ./sample.torrent --output ./out.bin
cargo run -p cli -- download 'magnet:?xt=urn:btih:...&tr=http://localhost:6969/announce' --output ./out.bin
```

Behavior:

- discovers peers from tracker
- performs auth + lock payment flow
- downloads pieces via x402 exchange
- reconstructs output file

### `tracker`

Convenience subcommand that launches tracker binary:

```bash
cargo run -p cli -- tracker --listen 0.0.0.0:6969
```

Equivalent environment behavior:

- sets `TRACKER_LISTEN`
- runs `cargo run --bin tracker --release`

## Tracker API

Default listen address: `0.0.0.0:6969`.

Endpoints:

- `GET /` and `GET /health`: health check
- `POST /announce`: register/update peer in swarm
- `GET /discover?info_hash=<hex>`: query peers only
- `POST /report`: reputation penalty signal
- `GET /stats`: swarm counts

Announce request fields:

- `info_hash` hex (20 bytes)
- `price` (u64)
- `peer_id` hex (20 bytes)
- `port`
- `pubkey` base58 (32 bytes)
- `left` (`0` => seeder, `>0` => leecher)
- optional `event` (`started`, `completed`, `stopped`, `update`)

Tracker state characteristics:

- in-memory only (no durable DB)
- stale peers cleaned every 5 minutes using 1-hour threshold
- default policy currently lenient (`min_stake = 0`)

## On-Chain Program (`x402-contract`)

Program ID:

- `CecHZhrZPyLZYFu1R3msJJEQeRDis83K3i99sRXydft3`

Instructions:

- `lock_payment(infohash, amount, merkle_root, total_secrets)`
- `claim_by_secrets(infohash, claims)`
- `release_payment(infohash)`
- `refund()`

Core account model:

- `PaymentEscrow`
  - leecher, seeder, infohash, vault, usdc_mint
  - amount, total_secrets, merkle_root, bump

PDA seeds:

- escrow: `["escrow_v2", leecher, seeder, infohash]`
- vault: `["vault", escrow]`

Claim logic summary:

- seeder submits multiple `{ index, secret, proof }`
- valid secrets are verified against escrow merkle root
- payout proportion is `valid_count / total_secrets`
- remaining funds return to leecher

## End-to-End Local Workflow

### 1) Build workspace

```bash
cargo build --release
```

### 2) Start tracker

```bash
cargo run -p x402-tracker --bin tracker
```

or

```bash
cargo run -p cli -- tracker --listen 0.0.0.0:6969
```

### 3) Create torrent or magnet

```bash
cargo run -p cli -- create ./test.txt --tracker http://localhost:6969/announce
```

### 4) Prepare seeder files

- Place `.torrent` in `./torrents/`
- Place payload file in `./data/` named by infohash (or infohash + extension)
- Add matching price entry in `./seeder.json`

### 5) Run seeder

```bash
cargo run -p cli -- serve --listen 0.0.0.0:6881 --tracker http://localhost:6969
```

### 6) Run leecher

```bash
cargo run -p cli -- download ./torrents/<hash>.torrent --output ./downloaded.bin
```

## Development and Testing

General Rust checks:

```bash
cargo fmt
cargo clippy --workspace --all-targets
cargo test --workspace
```

Tracker smoke test script:

```bash
bash tracker/test_tracker.sh
```

Anchor workspace commands (inside `x402-contract/`):

```bash
anchor build
anchor test
```

## Requirements and Tooling Notes

Minimum practical tooling for full stack experiments:

- Rust toolchain (workspace uses 2024 edition in core crates)
- Solana CLI + funded keypair at `~/.config/solana/id.json`
- Anchor CLI for contract workflows
- Node/Yarn for Anchor TS tests

Important compatibility notes observed in this repo:

- Keep workspace members pointed at `x402-contract/programs/*` (not root `x402-contract` as a Rust package target).
- `anchor-client` currently aligns with Solana SDK `2.3.x`; mixing direct `solana-sdk 4.x` types can cause trait/type mismatches.
- Anchor/Solana SBF toolchain versions may lag Rust edition requirements for some dependencies.

## Current Limitations

- Experimental protocol, not hardened for adversarial internet deployment.
- Tracker state is memory-only.
- Some magnet edge-cases (for example non-hex infohash forms) are not fully normalized through CLI download path.
- Torrent handling in this codebase targets single-file flows.
- Error handling is improving but still evolving in several peer-wire paths.

## Security and Production Disclaimer

This repository is for protocol research and learning.

- Not audited.
- Not production safe.
- Do not use with real funds or sensitive data outside controlled experimentation.

## Useful Files to Read Next

- `cli/src/main.rs`
- `x402-core/src/peer/leech.rs`
- `x402-core/src/peer/serve.rs`
- `x402-core/src/peer/protocol.rs`
- `tracker/src/tracker.rs`
- `x402-contract/programs/x402-contract/src/lib.rs`
