FROM rust:1.86.0 AS base
RUN apt-get update && apt-get install -y protobuf-compiler xxd
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

FROM base AS runtime
WORKDIR /app
ARG BUILD_MODE=debug
COPY --from=builder /app/target/${BUILD_MODE}/snowflake /app/
RUN mkdir /app/docker
COPY Makefile /app/
COPY docker/bootstrap.sh docker/common.sh docker/get_node_id.sh /app/docker/
COPY docker/entrypoint.sh /app/
RUN chmod +x /app/docker/bootstrap.sh /app/docker/common.sh /app/docker/get_node_id.sh /app/entrypoint.sh

ENTRYPOINT ["/app/entrypoint.sh"]