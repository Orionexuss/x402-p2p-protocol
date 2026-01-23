# x402 Tracker

Custom tracker server for the x402 P2P protocol with economic enforcement.

## Features

- **Peer Discovery**: Register and discover peers by info_hash
- **Economic Policy**: Stake and reputation requirements (Phase 2)
- **Reputation System**: Report misbehaving peers
- **Automatic Cleanup**: Remove stale peers after 1 hour
- **REST API**: JSON-based HTTP endpoints

## Running the Tracker

```bash
cargo run --bin tracker
```

The tracker will start on `0.0.0.0:6969`

## API Endpoints

### POST /announce
Register or update peer in a swarm.

**Request:**
```json
{
  "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
  "peer_id": "2d5834303230312d313233343536373839",
  "port": 6881,
  "pubkey": "0000000000000000000000000000000000000000000000000000000000000000",
  "uploaded": 0,
  "downloaded": 0,
  "left": 1024000,
  "pieces": [],
  "event": "started"
}
```

**Response:**
```json
{
  "interval": 1800,
  "min_stake": 0,
  "piece_price": 1000,
  "seeders": [],
  "leechers": [],
  "complete": 0,
  "incomplete": 1
}
```

### GET /discover?info_hash=<hex>
Query peers without registering.

### POST /report
Report a misbehaving peer.

```json
{
  "reporter": "2d5834303230312d313233343536373839",
  "reported": "2d5834303230312d393837363534333231",
  "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
  "reason": "invalid_data"
}
```

### GET /stats
Get tracker statistics.

```json
{
  "total_swarms": 1,
  "total_peers": 5,
  "total_seeders": 2,
  "total_leechers": 3
}
```

### GET /health
Health check endpoint.

## Configuration

Economic policy can be adjusted in `main.rs`:

```rust
let policy = EconomicPolicy {
    min_stake: 1_000_000,      // 0.001 SOL
    min_reputation: -100,
    penalty_threshold: -50,
};
```

## Phase 1 (Current)

- ✅ In-memory peer registry
- ✅ Announce/discover endpoints
- ✅ Reputation system
- ✅ Automatic peer cleanup
- ✅ Stats endpoint

## Phase 2 (Upcoming)

- [ ] Solana RPC integration
- [ ] Real stake verification
- [ ] Cryptographic signature verification
- [ ] On-chain proof validation

## Testing

```bash
# Start the tracker
cargo run --bin tracker

# Test announce (in another terminal)
curl -X POST http://localhost:6969/announce \
  -H "Content-Type: application/json" \
  -d '{
    "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
    "peer_id": "2d5834303230312d313233343536373839",
    "port": 6881,
    "pubkey": "0000000000000000000000000000000000000000000000000000000000000000",
    "left": 0
  }'

# Check stats
curl http://localhost:6969/stats
```
