FROM rust

COPY ./ ./

ARG RUST_LOG=info

RUN apt-get update && apt-get install -y libzmq3-dev
RUN cargo build --release

EXPOSE 4001

ENTRYPOINT ["target/release/RaftBlockChain"]
CMD ["-cconfig.json", "-ggenesis.json"]