#!/bin/bash
set -ex

source docker/common.sh

cd /shared/anr
go run examples/local/fivenodenetwork/main.go /shared/avalanchego