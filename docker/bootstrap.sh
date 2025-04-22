#!/bin/bash
set -ex

EXPECTED_PEERS=3
PEER_DIR=/shared/peer-info

echo "Waiting for $EXPECTED_PEERS peers to write their info..."

while [ "$(ls -1 $PEER_DIR | wc -l)" -lt "$EXPECTED_PEERS" ]; do
    sleep 1
done

echo "[" > /shared/peers.json
FIRST=true
for file in $PEER_DIR/*.json; do
    if [ "$FIRST" = true ]; then
        FIRST=false
    else
        echo "," >> /shared/peers.json
    fi
    cat "$file" >> /shared/peers.json
done
echo "]" >> /shared/peers.json

echo "peers.json created with $(ls $PEER_DIR | wc -l) peers."
