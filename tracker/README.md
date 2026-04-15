# x402 Tracker

HTTP tracker service for the x402 protocol.

The tracker handles swarm discovery, peer registration, and basic reputation signaling.
Current implementation is in-memory and optimized for local/dev testing.

## What It Does

- Registers peers into swarms by `info_hash`.
- Returns seeder/leecher lists for discovery.
- Applies lightweight reputation penalties via `report` requests.
- Cleans stale peers periodically.
- Exposes health and aggregate stats endpoints.

## Run

From repository root:

```bash
cargo run -p x402-tracker --bin tracker
```

Or from `tracker/`:

```bash
cargo run --bin tracker
```

Default bind address is controlled by `TRACKER_LISTEN` and falls back to:

```text
0.0.0.0:6969
```

Example custom listen:

```bash
TRACKER_LISTEN=127.0.0.1:8080 cargo run -p x402-tracker --bin tracker
```

## Endpoints

- `GET /` -> health text response
- `GET /health` -> health text response
- `POST /announce` -> register/update peer
- `GET /discover?info_hash=<hex>` -> query swarm peers
- `POST /report` -> report peer behavior
- `GET /stats` -> swarm totals

## API Details

### `POST /announce`

Registers or updates a peer in a swarm.

Request body:

```json
{
  "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
  "price": 1000000,
  "peer_id": "2d5834303230312d313233343536373839",
  "port": 6881,
  "pubkey": "11111111111111111111111111111111",
  "signature": "",
  "uploaded": 0,
  "downloaded": 0,
  "left": 1024000,
  "pieces": [],
  "event": "started"
}
```

Notes:

- `info_hash` must decode to 20 bytes (hex).
- `peer_id` must decode to 20 bytes (hex).
- `pubkey` must decode to 32 bytes (base58).
- `left == 0` means peer is placed in `seeders`; otherwise in `leechers`.
- `event` supports: `started`, `completed`, `stopped`, `update`.

Response body:

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

### `GET /discover?info_hash=<hex>`

Returns swarm state for an `info_hash` without registering caller as a peer.

Example:

```bash
curl "http://localhost:6969/discover?info_hash=d2474e86c95b19b8bcfdb92bc12c9d44667cfa36"
```

### `POST /report`

Applies a reputation penalty to a peer.

Request body:

```json
{
  "reporter": "2d5834303230312d313233343536373839",
  "reported": "2d5834303230312d393837363534333231",
  "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
  "reason": "invalid_data",
  "proof": "",
  "signature": ""
}
```

`reason` values:

- `invalid_data`
- `no_response`
- `payment_fraud`

### `GET /stats`

Returns aggregate tracker stats.

Example response:

```json
{
  "total_swarms": 1,
  "total_peers": 5,
  "total_seeders": 2,
  "total_leechers": 3
}
```

## Runtime Behavior

- Peer cleanup runs every 5 minutes.
- Peers not seen for 1 hour are removed.
- Empty swarms are removed automatically.
- CORS is currently open (`Any`) for development convenience.

## Economic/Reputation Policy

Default policy (`EconomicPolicy::default()`):

- `min_stake = 0`
- `min_reputation = -100`
- `penalty_threshold = -50`

Current stage:

- Stake verification is not enforced yet (stubbed for future phases).
- Reports decrement reputation by a fixed amount.

## Error Responses

HTTP status mapping:

- `400 Bad Request`: invalid hash/peer/pubkey formatting
- `401 Unauthorized`: invalid signature (future path)
- `403 Forbidden`: low reputation or insufficient stake
- `500 Internal Server Error`: internal tracker errors

Response shape:

```json
{
  "error": "<message>"
}
```

## Test Quickly

From repository root:

```bash
bash tracker/test_tracker.sh
```

Manual test snippet:

```bash
curl -s -X POST http://localhost:6969/announce \
  -H "Content-Type: application/json" \
  -d '{
    "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
    "price": 1000000,
    "peer_id": "2d5834303230312d313233343536373839",
    "port": 6881,
    "pubkey": "11111111111111111111111111111111",
    "left": 0,
    "event": "started"
  }'
```

## Limitations

- No persistent storage.
- No cryptographic announce signature verification yet.
- No real on-chain stake lookup enforcement yet.
- Designed for protocol experimentation, not production deployment.
