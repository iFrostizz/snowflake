FROM rust:1.86.0 AS base
RUN apt-get update && apt-get install -y protobuf-compiler xxd jq openssl
ENV GOLANG_VERSION=1.23.9
RUN curl -fsSL https://golang.org/dl/go${GOLANG_VERSION}.linux-amd64.tar.gz -o go.tar.gz && \
    tar -C /usr/local -xzf go.tar.gz && \
    rm go.tar.gz
ENV PATH="/usr/local/go/bin:${PATH}"
RUN cargo install cargo-chef sccache && \
    cargo install --git https://github.com/iFrostizz/bs58-rs --branch add-cb58

ENV RUSTC_WRAPPER=sccache
ENV CARGO_DIR=/usr/local/cargo/registry
ENV SCCACHE_DIR=/sccache
ENV GOCACHE=/root/.cache/go-cache
ENV GOMODCACHE=/root/.cache/gomod-cache

FROM base AS planner
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=$CARGO_DIR \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef prepare --recipe-path recipe.json

FROM base AS builder
ARG BUILD_MODE=debug
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=$CARGO_DIR \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    if [ "$BUILD_MODE" = "release" ]; then \
        cargo chef cook --release --recipe-path recipe.json; \
    else \
        cargo chef cook --recipe-path recipe.json; \
    fi
COPY . .
RUN --mount=type=cache,target=$CARGO_DIR \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    if [ "$BUILD_MODE" = "release" ]; then \
        cargo build --release; \
    else \
        cargo build; \
    fi

FROM base AS anr
WORKDIR /app
RUN git clone --depth 1 --branch custom_bin https://github.com/ifrostizz/avalanche-network-runner anr
WORKDIR /app/anr
COPY docker/keys/ /app/anr/local/default/
COPY docker/flags.json local/default/flags.json
RUN go env -w GOCACHE=$GOCACHE && \
    go env -w GOMODCACHE=$GOMODCACHE
RUN --mount=type=cache,target=$GOMODCACHE \
     go build ./examples/local/fivenodenetwork/main.go

FROM base AS avalanchego
WORKDIR /app
RUN git clone --depth 1 --branch v1.13.0 https://github.com/ava-labs/avalanchego
COPY . .
WORKDIR /app/avalanchego
RUN go env -w GOCACHE=$GOCACHE && \
    go env -w GOMODCACHE=$GOMODCACHE
RUN --mount=type=cache,target=$GOMODCACHE \
     ./scripts/build.sh

FROM base AS runtime
WORKDIR /app
ARG BUILD_MODE=debug

COPY --from=builder /app/target/${BUILD_MODE}/snowflake /app/
COPY Makefile /app/
COPY docker/*.sh /app/docker/
COPY docker/entrypoint.sh /app/
RUN chmod +x /app/docker/*.sh /app/entrypoint.sh
COPY --from=anr /app/anr/main /shared/anr/fivenodenetwork
COPY --from=avalanchego /app/avalanchego/build/avalanchego /shared/avalanchego

ENTRYPOINT ["/app/entrypoint.sh"]