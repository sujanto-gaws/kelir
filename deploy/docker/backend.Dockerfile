# Backend release image.
#
# Two stages so the shipped image carries the binary and nothing that built it.
# Migrations are embedded into the binary by `sqlx::migrate!` at compile time,
# so the runtime stage needs no migrations directory.
#
# Build with the commit so `/version` can report it (release process §4 step 7):
#   docker build -f deploy/docker/backend.Dockerfile \
#     --build-arg KELIR_BUILD_SHA=$(git rev-parse --short HEAD) \
#     -t kelir-backend:0.1.0 kelir-backend

FROM rust:1.89-slim-bookworm AS builder

WORKDIR /build

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Dependencies first, against a stub main, so a source-only change does not
# rebuild the whole dependency graph.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY build.rs ./
COPY migrations ./migrations
COPY src ./src

ARG KELIR_BUILD_SHA=unknown
ENV KELIR_BUILD_SHA=${KELIR_BUILD_SHA}

# Touch main.rs so cargo rebuilds it over the stub from the dependency layer.
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim AS runtime

# ca-certificates is needed for outbound TLS (SMTP, object storage, Phase 9
# integrations); curl gives the container a working HEALTHCHECK.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 kelir

COPY --from=builder /build/target/release/kelir-backend /usr/local/bin/kelir-backend

USER kelir
WORKDIR /home/kelir

EXPOSE 8080

# Liveness only: readiness depends on PostgreSQL, and a database outage must
# not make the orchestrator kill an otherwise healthy container.
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health/live || exit 1

ENTRYPOINT ["/usr/local/bin/kelir-backend"]
