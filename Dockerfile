# ---- build ---------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /app
# Cache dependencies first.
COPY Cargo.toml Cargo.lock build.rs ./
RUN mkdir -p src migrations && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && cargo build --release && rm -rf src
COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs src/lib.rs && cargo build --release

# ---- runtime ---------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home chimppos
COPY --from=build /app/target/release/chimpPOS_backend /usr/local/bin/chimpPOS_backend
USER chimppos
ENV HOST=0.0.0.0 PORT=3000 APP_ENV=production LOG_FORMAT=json
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:3000/health || exit 1
# Migrations are embedded in the binary: `chimpPOS_backend migrate` applies them.
ENTRYPOINT ["chimpPOS_backend"]
CMD ["serve"]
