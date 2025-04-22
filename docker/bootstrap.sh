#!/bin/bash
set -ex

EXPECTED_PEERS=3
PEER_DIR=/shared/peer-info
PEER_FILE=/shared/peers.json

rm $PEER_DIR/* >/dev/null 2>&1
rm $PEER_FILE >/dev/null 2>&1

echo "Waiting for $EXPECTED_PEERS peers to write their info..."

while [ "$(ls -1 $PEER_DIR | wc -l)" -lt "$EXPECTED_PEERS" ]; do
    sleep 1
done

echo "{\"mainnet\":[" > $PEER_FILE
FIRST=true
for file in $PEER_DIR/*.json; do
    if [ "$FIRST" = true ]; then
        FIRST=false
    else
        echo "," >> $PEER_FILE
    fi
    cat "$file" >> $PEER_FILE
done
echo "],\"fuji\": []}" >> $PEER_FILE

echo "peers.json created with $(ls $PEER_DIR | wc -l) peers."
