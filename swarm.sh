#!/bin/bash

set -e
trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM EXIT

n=$1

echo "generating ${n} keys..."

for i in $(seq 0 $((n - 1))); do
  dirname=/tmp/keys/keys_${i}
  if [ -d "$DIRECTORY" ]; then
    continue
  fi
  mkdir -p "$dirname"
  openssl req -x509 -newkey rsa:4096 -keyout "${dirname}"/node.key -out "${dirname}"/node.crt -days 36500 -nodes -subj '/CN=localhost' -set_serial 0 > /dev/null 2>&1 &
  openssl rand 32 > "${dirname}"/signer.key &
done

wait

echo "running swarm, hold ctrl + c to stop"

for i in $(seq 0 $((n - 1))); do
  echo "launching peer ${i}"
  dirname=/tmp/keys/keys_${i}
  cargo run -- --network-id fuji --max-peers 1 --http-port 0 --pem-key-path "${dirname}"/node.key --cert-path "${dirname}"/node.crt --bls-key-path "${dirname}"/signer.key > /dev/null 2>&1 &
  sleep 10
done

wait

trap 'quit=1' USR1

quit=0
while [ "$quit" -ne 1 ]; do
    printf 'Do "kill -USR1 %d" to exit this loop after the sleep\n' "$$"
    sleep 1
done

echo The USR1 signal has now been caught and handled