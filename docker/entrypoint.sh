#!/bin/bash
set -ex

LOG_LEVEL=debug
PEER_DIR=/shared/peer-info
PEER_FILE=/shared/peers.json

# Create keypair
make keys

# Derive NodeID from staker.crt
NODE_ID=$(bash ./docker/get_node_id.sh staker.crt)

# Use hostname to get a unique ID
PEER_ID=$(hostname | grep -oE '[0-9]+')
HTTP_PORT=$((3000 + PEER_ID))
RPC_PORT=$((4000 + PEER_ID))

echo "Peer $NODE_ID running on port $HTTP_PORT"

# Write info to shared dir
mkdir -p "$PEER_DIR"
echo "{\"id\": \"$NODE_ID\", \"ip\": \"127.0.0.1:$HTTP_PORT\"}" > "$PEER_DIR"/"$HOSTNAME".json

# Wait for the peer list
echo "Waiting for peer table..."
while [ ! -f "$PEER_FILE" ]; do
    sleep 1
done

# Start the app
RUST_LOG=$LOG_LEVEL cargo run -- --http-port $HTTP_PORT --rpc-port $RPC_PORT --max-peers 0 --light-bootstrappers-path "$PEER_FILE"
