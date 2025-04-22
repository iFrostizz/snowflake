FROM rust:1.86.0
WORKDIR /app
COPY . .

RUN apt-get update && apt-get install -y protobuf-compiler xxd
RUN cargo install --git https://github.com/iFrostizz/bs58-rs --branch add-cb58
RUN cargo build # --release

COPY docker/entrypoint.sh .
RUN chmod +x entrypoint.sh

ENTRYPOINT ["./entrypoint.sh"]
