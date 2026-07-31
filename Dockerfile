# A Rainier application, built and shipped — **sized to its deployment**.
#
# Two stages: a builder with the toolchain, and a runtime with neither the
# toolchain nor a shell's worth of userland. The result is the binary, the
# templates it renders, and nothing else — and the binary carries only the
# drivers the deployment's environment selects.
#
#   printf 'CACHE_DRIVER=redis\nMAIL_DRIVER=smtp\n' > .env.build
#   docker build -t rainier-sample .
#   docker build --build-arg ENV_FILE=.env.staging.build -t rainier-sample:staging .
#
#   docker run --rm -p 8000:8000 \
#     -e APP_KEY="$(openssl rand -base64 32 | sed 's/^/base64:/')" \
#     -e DATABASE_URL=sqlite:///data/app.sqlite?mode=rwc \
#     -v rainier-data:/data \
#     rainier-sample

# --- build -------------------------------------------------------------------

# Pinned, so an image built today and one built in six months are the same
# image. 1.94 rather than the framework's own 1.88 floor, because this image
# must be able to build *any* sized feature set and enabling a driver raises
# the floor — the AWS SDKs (`s3`, `sqs`) want 1.94.
FROM rust:1.94-bookworm AS builder

WORKDIR /build

# Which environment file sizes this image. `.env.build` by convention: the
# deployment's **driver selections and nothing secret** — this file lands in
# builder layers and the build cache, which is exactly where credentials must
# not, and the sizing needs only the selections. Secrets keep arriving at
# `docker run` as `-e`, the way the run command above shows. (`.env` itself
# is dockerignored for the same reason.)
#
# The default requires the file to exist, deliberately: an image sized
# without the deployment's selections would be sized wrong — the example's
# defaults would ship a log-mail, memory-cache binary — so a missing file
# fails the next COPY instead of shipping a documentation-shaped image.
ARG ENV_FILE=.env.build

# Manifests for both workspace members, and the xtask's (tiny) source: the
# feature computation runs before anything heavy so its answer can shape the
# cached dependency layer too. Stub sources stand in for the application so
# cargo can read the workspace without rebuilding three hundred crates every
# time `src/` changes.
COPY Cargo.toml Cargo.lock ./
COPY xtask/Cargo.toml xtask/Cargo.toml
COPY xtask/src xtask/src
COPY ${ENV_FILE} .env.build

RUN mkdir -p src \
 && echo 'fn main() {}' > src/main.rs \
 && echo '' > src/lib.rs

# Compute the feature set once, from the selections. `--list` is the
# scripting mode: the bare comma-separated set, and a selection nothing
# forwards fails the build here, loudly.
RUN cargo run --quiet --locked --package xtask -- features --env .env.build --list > .features \
 && echo "sized with features: [$(cat .features)]"

# Dependencies, in their own layer, with the *right* features — so the cache
# holds what the real build needs rather than a default set it rebuilds over.
RUN FEATURES="$(cat .features)" \
 && cargo build --release --locked --package app --no-default-features \
        ${FEATURES:+--features "$FEATURES"} \
 && rm -rf src

COPY src ./src
COPY resources ./resources

# `touch` because the stub above left cargo's fingerprint newer than the real
# sources it just replaced, and cargo would otherwise decide there is nothing
# to do and ship the stub.
RUN touch src/main.rs src/lib.rs \
 && FEATURES="$(cat .features)" \
 && cargo build --release --locked --package app --no-default-features \
        ${FEATURES:+--features "$FEATURES"} \
 && strip target/release/app

# --- runtime -----------------------------------------------------------------

# Debian slim rather than distroless or Alpine. Slim keeps the glibc the build
# linked against and the CA bundle an outbound HTTPS call needs; Alpine would
# mean musl and a second build; distroless would mean no shell, which is a real
# cost the first time something is wrong in production.
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install --no-install-recommends -y ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Not root. A container escape is one bug away from being a host compromise,
# and nothing here needs a privileged port.
RUN useradd --system --create-home --uid 10001 rainier

WORKDIR /app

COPY --from=builder /build/target/release/app /usr/local/bin/app
COPY --from=builder /build/resources ./resources

# The framework writes here: `storage/logs`, `storage/mail` for the file mail
# transport, `storage/app` for the local filesystem disk. `/data` is separate
# and is what you mount a volume at — see the note below.
RUN mkdir -p storage/logs storage/mail storage/app /data \
 && chown -R rainier:rainier /app /data

USER rainier

# `serve` reads `server.host` and `server.port` from the config, which read
# `SERVER_HOST` and `SERVER_PORT`. Binding to loopback inside a container makes
# it unreachable from outside, so the default has to be overridden here.
ENV SERVER_HOST=0.0.0.0 \
    SERVER_PORT=8000 \
    APP_ENV=production \
    RUST_LOG=info

EXPOSE 8000

# `/health` is a plain 200 with no database behind it, which is what a liveness
# probe should ask: "is this process serving?", not "is every dependency up?".
# A readiness probe that checks the database belongs in the orchestrator, where
# it can take the instance out of rotation without restarting it.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/app", "route:list"]

ENTRYPOINT ["/usr/local/bin/app"]
CMD ["serve"]
