FROM ekidd/rust-musl-builder as builder

ADD --chown=rust:rust . ./
RUN cargo build --release

FROM scratch

COPY --from=builder /home/rust/src/target/release/wwfypc-payments /

ENTRYPOINT /wwfypc-payments