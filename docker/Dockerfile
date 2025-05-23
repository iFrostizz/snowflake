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
ENV SCCACHE_DIR=/sccache

FROM base AS planner
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef prepare --recipe-path recipe.json

FROM base AS builder
ARG BUILD_MODE=debug
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    if [ "$BUILD_MODE" = "release" ]; then \
        cargo chef cook --release --recipe-path recipe.json; \
    else \
        cargo chef cook --recipe-path recipe.json; \
    fi
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    if [ "$BUILD_MODE" = "release" ]; then \
        cargo build --release; \
    else \
        cargo build; \
    fi

FROM base AS anr
WORKDIR /app

RUN git clone --depth 1 --branch v1.13.0 https://github.com/ava-labs/avalanchego && \
    cd avalanchego && \
    go mod download && \
    ./scripts/build.sh
RUN git clone --depth 1 --branch custom_bin https://github.com/ifrostizz/avalanche-network-runner anr
WORKDIR /app/anr
COPY docker/keys/ /app/anr/local/default/
COPY . .
RUN --mount=type=cache,target="/root/.cache/go-cache" go mod download

FROM base AS runtime
WORKDIR /app
ARG BUILD_MODE=debug

COPY --from=builder /app/target/${BUILD_MODE}/snowflake /app/
COPY Makefile /app/
COPY docker/*.sh /app/docker/
COPY docker/entrypoint.sh /app/
RUN chmod +x /app/docker/*.sh /app/entrypoint.sh
COPY --from=anr /app/avalanchego/build/avalanchego /shared/avalanchego
COPY --from=anr /app/anr/ /shared/anr/

ENTRYPOINT ["/app/entrypoint.sh"]