# A Rainier application, built and shipped.
#
# Two stages: a builder with the toolchain, and a runtime with neither the
# toolchain nor a shell's worth of userland. The result is the binary, the
# templates it renders, and nothing else.
#
#   docker build -t rainier-sample .
#   docker run --rm -p 8000:8000 \
#     -e APP_KEY="$(openssl rand -base64 32 | sed 's/^/base64:/')" \
#     -e DATABASE_URL=sqlite:///data/app.sqlite?mode=rwc \
#     -v rainier-data:/data \
#     rainier-sample

# --- build -------------------------------------------------------------------

FROM rust:1.85-bookworm AS builder

WORKDIR /build

# Dependencies first, in their own layer. Copying the manifests and building a
# stub means a change to `src/` does not rebuild three hundred crates — which
# is the difference between a twenty-second image and a five-minute one.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
 && echo 'fn main() {}' > src/main.rs \
 && echo '' > src/lib.rs \
 && cargo build --release --locked \
 && rm -rf src

COPY src ./src
COPY resources ./resources

# `touch` because the stub above left cargo's fingerprint newer than the real
# sources it just replaced, and cargo would otherwise decide there is nothing
# to do and ship the stub.
RUN touch src/main.rs src/lib.rs \
 && cargo build --release --locked \
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
