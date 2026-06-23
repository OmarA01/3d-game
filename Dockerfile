# Multi-stage build for the browser (WebAssembly) version of Crystal Rush,
# intended for DigitalOcean App Platform (or any Docker host).
#
#   Stage 1 (builder): full Rust toolchain compiles the wasm.
#   Stage 2 (runtime): a tiny nginx that serves the static web/ files.
#
# App Platform autodetects this Dockerfile and runs the final image as a
# Service. Because the game is 100% client-side (no backend), the "service" is
# just nginx serving four static files; the smallest instance size is plenty.

# ----------------------------------------------------------------- build wasm
FROM rust:1-bookworm AS builder

# The wasm target + the linker flag macroquad needs at runtime (Rust >=1.96
# stopped passing --allow-undefined by default; the JS bundle provides the
# imported host functions in the browser).
RUN rustup target add wasm32-unknown-unknown
ENV CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=--allow-undefined"

WORKDIR /app
COPY . .

# Build and drop the artifact next to the committed web/ shell (index.html +
# JS shims), producing a complete static site under /app/web.
RUN cargo build --release --target wasm32-unknown-unknown \
    && cp target/wasm32-unknown-unknown/release/crystal-rush.wasm web/crystal-rush.wasm

# -------------------------------------------------------------- serve static
FROM nginx:alpine

# Serve the assembled site. .wasm must be sent as application/wasm or the
# browser refuses to stream-instantiate it; nginx's default mime.types in
# recent images includes it, but we set it explicitly to be safe.
COPY --from=builder /app/web /usr/share/nginx/html
RUN printf 'types { application/wasm wasm; }\n' > /etc/nginx/conf.d/wasm-mime.conf || true

# App Platform routes to $PORT (default 8080); make nginx listen there.
RUN sed -i 's/listen\s*80;/listen 8080;/' /etc/nginx/conf.d/default.conf
EXPOSE 8080

CMD ["nginx", "-g", "daemon off;"]
