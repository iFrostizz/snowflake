FROM rust:1.86.0 AS base
RUN cargo install cargo-chef

FROM base AS planner
WORKDIR /app
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM base AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build

FROM base AS deps
WORKDIR /app
RUN apt-get update && apt-get install -y protobuf-compiler xxd
RUN cargo install --git https://github.com/iFrostizz/bs58-rs --branch add-cb58
COPY . .
#COPY docker/entrypoint.sh .
RUN chmod +x docker/entrypoint.sh

ENTRYPOINT ["./docker/entrypoint.sh"]
