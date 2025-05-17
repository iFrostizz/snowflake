#!/bin/bash
set -ex

source docker/common.sh

export GOCACHE=/root/.cache/go-cache
export GOMODCACHE=/root/.cache/gomod-cache

cd /shared/anr
go run examples/local/fivenodenetwork/main.go /shared/avalanchego