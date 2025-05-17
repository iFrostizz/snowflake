#!/bin/bash
set -ex

export EXPECTED_PEERS=3
export LOG_LEVEL=debug
export PEER_DIR=/shared/peer-info
export PEER_FILE=/shared/peers.json
export BOOTSTRAPPERS_FILE=/shared/bootstrappers.json
