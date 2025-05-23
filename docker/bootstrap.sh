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

echo "{\"local\":[" > "$PEER_FILE"
FIRST=true
for file in "$PEER_DIR"/*.json; do
  if [ "$FIRST" = true ]; then
    FIRST=false
  else
    echo "," >> "$PEER_FILE"
  fi
  cat "$file" >> "$PEER_FILE"
done
echo "],\"mainnet\":[],\"fuji\":[]}" >> "$PEER_FILE"

get_node_id() {
  port=$1
  node_id=$(curl -s -X POST --data '{
      "jsonrpc":"2.0",
      "id"     :1,
      "method" :"info.getNodeID"
  }' -H 'content-type:application/json;' 127.0.0.1:"$port"/ext/info | jq -r '.result.nodeID')
  echo "$node_id"
}

NODE_ID1=$(get_node_id 9650)
NODE_ID2=$(get_node_id 9652)
NODE_ID3=$(get_node_id 9654)
NODE_ID4=$(get_node_id 9656)
NODE_ID5=$(get_node_id 9658)

json_content="{
  \"local\":[
    {
      \"id\":\"$NODE_ID1\",
      \"ip\":\"127.0.0.1:9651\"
    },
    {
      \"id\":\"$NODE_ID2\",
      \"ip\":\"127.0.0.1:9653\"
    },
    {
      \"id\":\"$NODE_ID3\",
      \"ip\":\"127.0.0.1:9655\"
    },
    {
      \"id\":\"$NODE_ID4\",
      \"ip\":\"127.0.0.1:9657\"
    },
    {
      \"id\":\"$NODE_ID5\",
      \"ip\":\"127.0.0.1:9659\"
    }
  ],
  \"mainnet\":[],
  \"fuji\":[]
}"

echo "${json_content}" > "$BOOTSTRAPPERS_FILE"

echo "peers.json created with $(find "$PEER_DIR" -maxdepth 1 -type f | wc -l) peers."