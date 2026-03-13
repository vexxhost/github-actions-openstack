FROM rust:1.94.0@sha256:7e322aa1b876cbb977e0df46812af6c4e8be2efbfb2ce3712c28a93ba2968726 AS base
RUN cargo install --locked cargo-chef sccache
ENV RUSTC_WRAPPER=sccache SCCACHE_DIR=/sccache

FROM base AS planner
WORKDIR /app
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM base AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo build --release

FROM base AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/github-actions-openstack /usr/local/bin/github-actions-openstack
ENTRYPOINT ["/usr/local/bin/github-actions-openstack"]
