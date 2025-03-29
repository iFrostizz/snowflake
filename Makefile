.PHONY: proto

export OUT_DIR=proto/src/

ci: fmt	clippy test proto-check

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

keys:
	openssl req -x509 -newkey rsa:4096 -keyout staker.key -out staker.crt -days 36500 -nodes -subj '/CN=localhost' -set_serial 0
	openssl rand 32 > bls.key

proto:
	cargo build -p proto

proto-check: proto
	git diff --quiet -- proto/src/

metrics:
	sudo prometheus --config.file ./prometheus.yml --web.listen-address=:9898

pg_reset:
	psql -U admin -c "$(DATABASE_KICK)"
	psql -U admin -c "DROP database snowflake;"
	psql -U admin -c "CREATE database snowflake;"
	sqlx database reset
