#!/bin/bash

# Test script for x402 tracker

BASE_URL="http://localhost:6969"

echo "=== Testing x402 Tracker ==="
echo

# Test 1: Health check
echo "1. Health Check"
curl -s $BASE_URL/health
echo -e "\n"

# Test 2: Announce as seeder
echo "2. Announce as Seeder"
curl -s -X POST $BASE_URL/announce \
  -H "Content-Type: application/json" \
  -d '{
    "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
    "peer_id": "2d5834303230312d313233343536373839",
    "port": 6881,
    "pubkey": "1111111111111111111111111111111111111111111111111111111111111111",
    "left": 0,
    "event": "started"
  }' | jq
echo

# Test 3: Announce as leecher
echo "3. Announce as Leecher"
curl -s -X POST $BASE_URL/announce \
  -H "Content-Type: application/json" \
  -d '{
    "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
    "peer_id": "2d5834303230312d393837363534333231",
    "port": 6882,
    "pubkey": "2222222222222222222222222222222222222222222222222222222222222222",
    "left": 1024000,
    "event": "started"
  }' | jq
echo

# Test 4: Discover peers
echo "4. Discover Peers"
curl -s "$BASE_URL/discover?info_hash=d2474e86c95b19b8bcfdb92bc12c9d44667cfa36" | jq
echo

# Test 5: Stats
echo "5. Tracker Stats"
curl -s $BASE_URL/stats | jq
echo

# Test 6: Report peer
echo "6. Report Misbehaving Peer"
curl -s -X POST $BASE_URL/report \
  -H "Content-Type: application/json" \
  -d '{
    "reporter": "2d5834303230312d313233343536373839",
    "reported": "2d5834303230312d393837363534333231",
    "info_hash": "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36",
    "reason": "invalid_data"
  }'
echo -e "\n"

echo "=== Tests Complete ==="
