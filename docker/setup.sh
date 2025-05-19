#!/bin/bash
set -ex

source docker/common.sh

export GOCACHE=/root/.cache/go-cache
export GOMODCACHE=/root/.cache/gomod-cache

cd /shared/anr
echo '{
        "network-max-reconnect-delay":"1s",
        "public-ip":"127.0.0.1",
        "health-check-frequency":"2s",
        "api-admin-enabled":true,
        "index-enabled":true,
        "log-display-level":"DEBUG",
        "log-level": "DEBUG"
      }
' > local/default/flags.json

go run examples/local/fivenodenetwork/main.go /shared/avalanchego