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
# It is also the one place `CACHE_DRIVER`, `QUEUE_DRIVER` and `STORAGE_DRIVER`
# are still spelled, and that is worth knowing before it confuses somebody. The
# application reads none of them: a store, a queue connection and a disk each
# name their own driver inside `config/cache.rs`, `config/queue.rs` and
# `config/storage.rs`.
#
# `cargo rainier features` still needs them because it cannot read those
# sections, and that is a boundary rather than a gap: a section is declared in
# **Rust**, by this application's own `configure`, so reading one would mean
# running the application — which a tool deciding how to *build* it cannot do.
# Rainier's own `docs/deployment.md` covers it under "Sizing the image"; what
# follows is only what it means for *this* Dockerfile.
#
# The two environments never meet. This file is copied into the *builder* and
# nothing copies it into the runtime stage below, which matters most for the two
# that are refused at boot:
#
#   CACHE_DRIVER, QUEUE_DRIVER   boot failure in the container — keep them here
#   STORAGE_DRIVER               read by nothing at runtime; needed here for an
#                                `s3` disk, which has nothing else to announce it
#
# `STORAGE_DRIVER` is the one that silently *under*-builds: leave it out and a
# deployment whose `filesystems` section declares an S3 disk gets an image with
# no S3 client in it, and finds out on the first upload.
#
# The default requires the file to exist, deliberately: an image sized
# without the deployment's selections would be sized wrong — the example's
# defaults would ship a log-mail, memory-cache binary — so a missing file
# fails its COPY instead of shipping a documentation-shaped image.
ARG ENV_FILE=.env.build

# The sizing tool, pinned to the same framework revision the application's
# lockfile pins — so the driver→feature mapping the image is sized with is
# the one the application compiles against. This layer re-runs only when
# that revision moves.
COPY Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    REV=$(sed -n 's|^source = "git+https://github.com/safewords/rainier-framework.git#\(.*\)"|\1|p' Cargo.lock | head -n1) \
 && cargo install cargo-rainier --locked \
      --git https://github.com/safewords/rainier-framework.git --rev "$REV"

COPY Cargo.toml ./
COPY src ./src
COPY resources ./resources
COPY ${ENV_FILE} .env.build

# Compute the feature set — after the real sources are present, because the
# set includes what the code reaches for (`Jwt`, the `Http` facade), not only
# what the environment selects. `--list` is the scripting mode: the bare
# comma-separated set, and a selection nothing forwards fails the build here,
# loudly.
RUN cargo rainier features --env .env.build --list > .features \
 && echo "sized with features: [$(cat .features)]"

# BuildKit cache mounts do what a stub-crate dance used to: the registry and
# target caches persist across builds, so a source-only change recompiles the
# application rather than three hundred dependencies — without a fake
# `main.rs` that can be shipped by fingerprint accident. The binary is copied
# out because a cache mount is not part of the layer.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    FEATURES="$(cat .features)" \
 && cargo build --release --locked --no-default-features \
        ${FEATURES:+--features "$FEATURES"} \
 && strip target/release/app \
 && cp target/release/app /usr/local/bin/app

# --- assets ------------------------------------------------------------------

# The Vite build, in its own stage so the Rust cache and the npm cache never
# invalidate each other. What comes out is `public/build` — content-hashed
# files plus the manifest the `@vite` directive resolves entries against.
FROM node:22-bookworm-slim AS assets

WORKDIR /build

COPY package.json package-lock.json vite.config.js ./
RUN --mount=type=cache,target=/root/.npm npm ci

COPY resources ./resources
RUN npm run build

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

COPY --from=builder /usr/local/bin/app /usr/local/bin/app
COPY --from=builder /build/resources ./resources
# The compiled frontend, served by the `/build/{path*}` route and resolved by
# `@vite` through its manifest.
COPY --from=assets /build/public/build ./public/build

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
