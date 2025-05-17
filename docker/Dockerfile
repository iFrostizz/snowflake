FROM rust:1.86.0 AS base
RUN apt-get update && apt-get install -y protobuf-compiler xxd jq
ENV GOLANG_VERSION=1.23.9
RUN curl -fsSL https://golang.org/dl/go${GOLANG_VERSION}.linux-amd64.tar.gz -o go.tar.gz && \
    tar -C /usr/local -xzf go.tar.gz && \
    rm go.tar.gz
ENV PATH="/usr/local/go/bin:${PATH}"
RUN cargo install cargo-chef
RUN cargo install sccache
RUN cargo install --git https://github.com/iFrostizz/bs58-rs --branch add-cb58
ENV RUSTC_WRAPPER=sccache SCCACHE_DIR=/sccache

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
RUN git clone https://github.com/ava-labs/avalanchego avalanchego
WORKDIR /app/avalanchego
RUN go mod download
COPY . .
ENV GOCACHE=/root/.cache/go-cache
ENV GOMODCACHE=/root/.cache/gomod-cache
RUN --mount=type=cache,target="/root/.cache/go-cache" ./scripts/build.sh
WORKDIR /app
RUN git clone https://github.com/ifrostizz/avalanche-network-runner anr
WORKDIR /app/anr
COPY . .
RUN --mount=type=cache,target="/root/.cache/go-cache" go mod download

FROM base AS runtime
WORKDIR /app
ARG BUILD_MODE=debug
COPY --from=builder /app/target/${BUILD_MODE}/snowflake /app/
RUN mkdir /app/docker
COPY Makefile /app/
COPY docker/bootstrap.sh docker/setup.sh docker/common.sh docker/get_node_id.sh docker/check_anr_ready.sh /app/docker/
COPY docker/entrypoint.sh /app/
COPY --from=anr /app/avalanchego/build/avalanchego /shared/avalanchego
COPY --from=anr /app/anr/ /shared/anr/
RUN chmod +x /app/docker/bootstrap.sh /app/docker/common.sh /app/docker/get_node_id.sh /app/docker/check_anr_ready.sh /app/entrypoint.sh
ENTRYPOINT ["tail", "-f", "/dev/null"]

ENTRYPOINT ["/app/entrypoint.sh"]