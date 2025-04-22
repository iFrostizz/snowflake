#!/bin/bash
set -e

# Create keypair
make keys

# Derive NodeID from staker.crt
NODE_ID=$(cargo run -- get-node-id --cert staker.crt)

# Use hostname to get a unique ID
PEER_ID=$(hostname | grep -oE '[0-9]+')
PORT=$((3000 + PEER_ID))

echo "Peer $NODE_ID running on port $PORT"

# Write info to shared dir
mkdir -p /shared/peer-info
echo "{\"node_id\": \"$NODE_ID\", \"ip\": \"127.0.0.1\", \"port\": $PORT}" > /shared/peer-info/"$HOSTNAME".json

# Wait for the peer list
echo "Waiting for peer table..."
while [ ! -f /shared/peers.json ]; do
    sleep 1
done

# Start the app
cargo run -- --port $PORT --peers /shared/peers.json
