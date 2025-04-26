#!/bin/bash
set -ex

source docker/common.sh

mkdir -p "$PEER_DIR"
if ! rm -f "$PEER_DIR"/* >/dev/null 2>&1 || ! rm -f "$PEER_FILE" >/dev/null 2>&1; then
    echo "Warning: Could not fully clean up old peer data" >&2
fi

echo "Waiting for $EXPECTED_PEERS peers to write their info..."

while [ "$(find "$PEER_DIR" -maxdepth 1 -type f | wc -l)" -lt "$EXPECTED_PEERS" ]; do
  sleep 1
done

echo "{\"mainnet\":[" > "$PEER_FILE"
FIRST=true
for file in "$PEER_DIR"/*.json; do
  if [ "$FIRST" = true ]; then
    FIRST=false
  else
    echo "," >> "$PEER_FILE"
  fi
  cat "$file" >> "$PEER_FILE"
done
echo "],\"fuji\":[]}" >> "$PEER_FILE"

echo "{\"mainnet\":[],\"fuji\":[]}" > "$BOOTSTRAPPERS_FILE"

echo "peers.json created with $(find "$PEER_DIR" -maxdepth 1 -type f | wc -l) peers."