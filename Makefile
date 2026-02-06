.PHONY: proto, keys

ci-check: fmt-check	clippy test proto-check

ci-fix: fmt	clippy test proto

fmt:
	cargo fmt -p node

fmt-check:
	cargo fmt -p node --check

clippy:
	cargo clippy -p node -- -D warnings

test:
	cargo test --quiet

keys:
	openssl ecparam -name prime256v1 -genkey -noout -out staker.key
	openssl pkcs8 -topk8 -nocrypt -in staker.key -out staker.key.tmp
	mv staker.key.tmp staker.key
	openssl req -x509 -new -key staker.key -out staker.crt -days 36500 -subj '/CN=localhost' -set_serial 0
	openssl rand 32 > signer.key

proto:
	cargo build -p proto

proto-check: proto
	git diff --quiet -- proto/src/

metrics:
	sudo prometheus --config.file ./prometheus.yml --web.listen-address=:9898
