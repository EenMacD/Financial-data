# syntax=docker/dockerfile:1

# ---------- Stage 1: build the Vite/TS frontend ----------
# Pin linux/amd64 (matches the runtime + Cloud Run). Avoid the BuildKit-only
# $BUILDPLATFORM arg — Cloud Build's classic docker builder leaves it empty,
# producing an invalid `--platform=` and failing at the first FROM.
FROM --platform=linux/amd64 node:20-slim AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm ci || npm install
COPY frontend/ ./
RUN npm run build          # -> /app/frontend/dist

# ---------- Stage 2: build the Rust backend ----------
# Cloudflare Containers run on linux/amd64, so produce an amd64 binary
# (emulated via QEMU on Apple Silicon — slower, but correct).
FROM --platform=linux/amd64 rust:1.94-slim-bookworm AS backend
WORKDIR /app/backend
COPY backend/ ./
RUN cargo build --release  # -> /app/backend/target/release/money-data-backend

# ---------- Stage 3: slim runtime ----------
FROM --platform=linux/amd64 debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend  /app/backend/target/release/money-data-backend /usr/local/bin/money-data-backend
COPY --from=frontend /app/frontend/dist /app/frontend_dist

ENV FRONTEND_DIR=/app/frontend_dist
ENV PORT=8080
EXPOSE 8080
CMD ["money-data-backend"]
