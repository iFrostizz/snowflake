#!/bin/bash
set -e

source docker/common.sh

# Create keypair
make keys

# Derive NodeID from node.crt
NODE_ID=$(bash ./docker/get_node_id.sh node.crt)

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

# Enable backtrace and set more verbose logging
export RUST_BACKTRACE=1
export RUST_LOG=info,snowflake=${LOG_LEVEL:-debug}

/app/snowflake --public-ip 127.0.0.1 --http-port $HTTP_PORT --rpc-port $RPC_PORT \
  --max-peers 0 --bootstrappers-path "$BOOTSTRAPPERS_FILE" --light-bootstrappers-path "$PEER_FILE"
