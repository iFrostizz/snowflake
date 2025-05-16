#!/bin/bash
set -ex

for PORT in 9650 9652 9654 9656 9658; do
  healthy=$(curl -s -H 'Content-Type: application/json' --data '{
    "jsonrpc":"2.0",
    "id":1,
    "method":"health.readiness",
    "params": { "tags": [] }
  }' http://localhost:$PORT/ext/health | jq -r '.result.healthy')

  if [ "$healthy" != "true" ]; then
    exit 1
  fi
done

exit 0
