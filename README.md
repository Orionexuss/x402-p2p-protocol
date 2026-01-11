# x402 Protocol

x402 is a **research-oriented peer-to-peer (P2P) protocol** inspired by BitTorrent, built to explore **payment-enforced data exchange** using cryptographic primitives and on-chain settlement (Solana).

This project focuses on **protocol-level understanding and design**, not production deployment.

---

## Why x402 Exists

BitTorrent is one of the most successful distributed protocols ever created.  
However, it relies almost entirely on **voluntary cooperation**:

- Peers may leave immediately after downloading
- Long-term seeders are poorly incentivized
- There is no native economic settlement

x402 explores a different approach:

> **Data is only delivered if value is exchanged.**

This project investigates how to:
- Preserve BitTorrent's efficiency and robustness
- Add **cryptographic, verifiable incentives**
- Enforce payment at the protocol layer instead of relying on reputation

---

## Core Idea

x402 keeps **BitTorrent's metadata model intact** but extends the **peer wire protocol** with payment-aware semantics.

- Metadata remains content-addressed
- Data remains piece-based
- Integrity remains cryptographically verifiable
- **Peers serve pieces only after proof of payment**

---

## High-Level Architecture

```
+---------------------+
| CLI Client          |
| ------------------- |
| - Bencode parser    |
| - Torrent creator   |
| - Peer connections  |
| - Piece requests    |
+----------+----------+
           |
           v
+---------------------+
| P2P Network         |
| ------------------- |
| - Paid piece flow   |
| - Concurrent peers  |
| - Integrity checks  |
+----------+----------+
           |
           v
+---------------------+
| Solana Program      |
| ------------------- |
| - Escrow            |
| - Settlement logic  |
| - Reward distrib.   |
+---------------------+
```

---

## Torrent Files in x402

### Does x402 Change the `.torrent` Format?

**No.**

The `.torrent` file structure remains **100% BitTorrent-compatible**.

This is intentional.

### Why Keep It the Same?

* Infohash remains deterministic
* Existing tooling still works
* The file hash still uniquely identifies content
* Compatibility allows gradual protocol evolution

x402 modifies **how peers interact**, not how content is described.

---

## `.torrent` File Structure (Quick Overview)

A torrent file is a **bencoded dictionary** with two key sections:

```text
{
  "announce": "<tracker url>",
  "info": {
    "name": "<file or folder name>",
    "piece length": <integer>,
    "pieces": <concatenated SHA1 hashes>,
    "length": <file size> OR "files": [...]
  }
}
```

### Critical Rule

The **infohash** is computed as:

```
SHA1( bencode(info_dictionary) )
```

Byte-perfect encoding matters.

---

## How `.torrent` Files Are Created (Real BitTorrent)

1. User selects a file or folder
2. Data is split into fixed-size pieces
3. Each piece is SHA1-hashed
4. Hashes are concatenated into `pieces`
5. Metadata is bencoded
6. Result is saved as `.torrent`

x402 implements this exact process.

---

## Peer Interaction (Baseline BitTorrent)

1. Client parses `.torrent`
2. Computes infohash
3. Contacts tracker or DHT
4. Receives peer list
5. Connects to peers
6. Performs handshake
7. Requests pieces by index
8. Verifies SHA1 integrity

---

## How x402 Extends This Flow

The **connection flow is preserved**, but message semantics change:

| Step           | BitTorrent        | x402                   |
| -------------- | ----------------- | ---------------------- |
| Handshake      | Infohash exchange | Same                   |
| Interested     | Yes               | Yes                    |
| Piece Request  | Free              | Requires payment proof |
| Piece Response | Immediate         | Conditional            |
| Settlement     | None              | On-chain               |

Peers are no longer altruistic participants.
They are **economic actors**.

---

## Payment Model (Conceptual)

* Downloader escrows funds
* Each piece is priced
* Peer validates payment proof
* Piece is sent
* Final settlement occurs on-chain
* Rewards are redistributed to serving peers

This is **not a storage blockchain**, but a **paid transport layer**.

---

## Educational Value

This project provides deep hands-on exposure to:

* Distributed systems
* Protocol design
* P2P networking
* Cryptographic content addressing
* Economic incentive mechanisms
* Systems programming in Rust
* Blockchain integration at the networking layer

---

## What This Project Is NOT

* A commercial protocol
* A full blockchain storage network
* A BitTorrent replacement
* A production-ready system

This is **protocol research**, intentionally low-level and explicit.

---

## Getting Started

### Prerequisites

- Rust (latest stable)
- Cargo

### Build

```bash
cargo build --release
```

### Run the CLI

```bash
# Start a seeder
cargo run --bin cli -- serve

# Parse a torrent file
cargo run --bin cli -- parse <torrent-file>
```