FROM rust:1.86.0
WORKDIR /app
COPY .. .

RUN apt-get update && apt-get install -y protobuf-compiler
RUN cargo build # --release

COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

ENTRYPOINT ["./entrypoint.sh"]
