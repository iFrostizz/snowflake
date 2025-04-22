#!/bin/bash

Help() {
  echo "get_node_id.sh ~ Generate an Avalanche node ID from a cert file"
  echo
  echo "Syntax: get_node_id.sh ./staker.crt"
  echo
  exit 1
}

Install() {
  echo "please install with: cargo install --git https://github.com/iFrostizz/bs58-rs --branch add-cb58"
  exit 1
}

FILE=$1
if [ -z "$FILE" ]
then
  Help
fi

if ! [ -e "$FILE" ]
then
  echo "error: File does not exist."
  echo
  Help
fi

if ! command -v bs58 >/dev/null 2>&1; then
  echo "error: bs58-cli was not found"
  echo
  Install
elif ! echo | bs58 --cb58 >/dev/null 2>&1; then
  echo "error: bs58-cli was found but the --cb58 is not available"
  echo
  Install
fi

# Convert PEM to DER and hash the DER
SHA=$(openssl x509 -in "$FILE" -outform der | openssl sha256 | awk '{print $2}')

# Hash the SHA256 again using RIPEMD-160
RMD=$(echo -n "$SHA" | xxd -r -p | openssl rmd160 | awk '{print $2}')

# Convert to CB58 (you need the 'bs58' tool installed and it should support --cb58)
NODE_ID_CB58=$(echo "$RMD" | xxd -r -p | bs58 --cb58)
NODE_ID="NodeID-$NODE_ID_CB58"
echo "$NODE_ID"

