.PHONY: proto

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
	openssl req -x509 -newkey rsa:4096 -keyout node.key -out node.crt -days 36500 -nodes -subj '/CN=localhost' -set_serial 0
	openssl rand 32 > bls.key

proto:
	cargo build -p proto

proto-check: proto
	git diff --quiet -- proto/src/

metrics:
	sudo prometheus --config.file ./prometheus.yml --web.listen-address=:9898
