# ---- Builder stage ----
FROM rust:bookworm AS builder

# Install build deps for tree-sitter (C grammars) and rusqlite (bundled SQLite)
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency build: copy manifests and build with a dummy main
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() { println!("stub"); }' > src/main.rs
RUN cargo build --release && rm -rf src target/release/deps/bexp*

# Copy real source and build
COPY src/ src/
RUN cargo build --release

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root user
RUN groupadd --system bexp && useradd --system --gid bexp --create-home bexp

COPY --from=builder /build/target/release/bexp /usr/local/bin/bexp

# Workspace mount point
RUN mkdir /workspace && chown bexp:bexp /workspace
VOLUME ["/workspace"]

USER bexp

ENTRYPOINT ["/usr/local/bin/bexp"]
CMD ["serve", "--workspace", "/workspace"]
