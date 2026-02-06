#!/bin/bash
set -ex

source docker/common.sh

# Use hostname to get a unique ID
PEER_ID=$(hostname | grep -oE '[0-9]+')
HTTP_PORT=$((3000 + PEER_ID))
RPC_PORT=$((4000 + PEER_ID))

KEY_DIR="/app/docker/keys/node${PEER_ID}"
if [ ! -d "$KEY_DIR" ]; then
    echo "Missing key directory: $KEY_DIR" >&2
    exit 1
fi
if [ ! -f "$KEY_DIR/staker.key" ] || [ ! -f "$KEY_DIR/staker.crt" ]; then
    echo "Missing staker key/cert in $KEY_DIR" >&2
    exit 1
fi
cp "$KEY_DIR/staker.key" /app/node.key
cp "$KEY_DIR/staker.crt" /app/node.crt
if [ -f "$KEY_DIR/bls.key" ]; then
    cp "$KEY_DIR/bls.key" /app/bls.key
elif [ -f /app/bls.key ]; then
    echo "Using existing /app/bls.key"
else
    echo "Missing bls.key in $KEY_DIR and /app" >&2
    exit 1
fi

# Derive NodeID from node.crt
NODE_ID=$(bash ./docker/get_node_id.sh node.crt)

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
  --bootstrappers-path "$BOOTSTRAPPERS_FILE" ${PEER_ARGS:+"$PEER_ARGS"} --network-id local
