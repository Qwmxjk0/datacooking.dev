FROM rust:1-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=builder /app/target/release/datacooking /app/datacooking
COPY static ./static
EXPOSE 3000
ENV RUST_LOG=info
ENV PORT=3000
ENV STATIC_DIR=/app/static
CMD ["/app/datacooking"]
