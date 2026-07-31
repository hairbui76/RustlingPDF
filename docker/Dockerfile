# syntax=docker/dockerfile:1.7
# RustlingPDF container image.
#
# Targets (see docs in rust/RUNNING_WITH_RUST.md, "Docker" section):
#   default (runtime)  — the full self-hosted image: rustling-processing release
#                        binary + baked-in SPA dist + PDFium + the external
#                        conversion tools, serving everything on :8080.
#   ai-engine          — a slim optional sidecar with only the rustling-ai-engine
#                        binary (AI features), serving on :5001. Built from the
#                        same Dockerfile so the two share one Rust build stage:
#                        `docker build --target ai-engine -f docker/Dockerfile .`
#
# Build from the repository root:
#   docker build -t rustlingpdf -f docker/Dockerfile .
#
# Dependency caching: the Rust stage uses BuildKit cache mounts for the cargo
# registry and the target dir instead of cargo-chef-style dummy-main layer
# tricks — the workspace has four crates with build scripts and include_str!
# resources, which makes manifest-only dependency layers brittle. Cache mounts
# give the same incremental-rebuild property on any BuildKit builder (Docker
# >= 23 default). The npm stage uses classic layer caching on the lockfile.
#
# OCI metadata build args (all optional — a plain `task docker:build` /
# `docker build` with no --build-arg keeps working): the release workflow
# (.github/workflows/release.yml) passes the real values; local builds fall
# back to the neutral defaults below. Consumed as LABELs by both final
# targets (runtime and ai-engine).

ARG VERSION=dev
ARG REVISION=unknown
ARG CREATED=unknown
ARG SOURCE=https://github.com/hairbui76/RustlingPDF

########################################################################
# Stage 1: Rust release build (both binaries, one shared stage)
########################################################################
# rust-version in rust/Cargo.toml is 1.94; trixie matches the runtime stage's
# glibc so release binaries built here run unmodified on debian:trixie-slim.
FROM rust:1.94-slim-trixie AS rust-build

# Native -sys build deps (C toolchain for ring and friends; cmake/perl kept
# for -sys crates that probe for them).
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential cmake perl pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY rust/ rust/

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/rust/target,sharing=locked \
    cd rust \
    && cargo build --release --locked \
         -p rustling-processing --bin rustling-processing \
         -p rustling-ai-engine --bin rustling-ai-engine \
    && install -D -m 0755 target/release/rustling-processing /out/rustling-processing \
    && install -D -m 0755 target/release/rustling-ai-engine  /out/rustling-ai-engine

########################################################################
# Stage 2: Frontend production build (Vite, proprietary flavor)
########################################################################
# Flavor decision: upstream Stirling-PDF's self-hosted embedded image builds
# with RUSTLING_FLAVOR=proprietary (docker/embedded/Dockerfile, the default),
# and this repository's own default dev/build mode is proprietary too
# (vite.config.ts falls back to "proprietary"; `task frontend:dev` runs it).
# The proprietary build degrades to core behavior at runtime via
# /api/v1/config/app-config flags (login/premium stay off in open mode), so it
# is the closest match to upstream's shipped self-hosted web UI.
#
# Node 22: vite ^7.3 requires ^20.19 || >=22.12 (same reasoning as CI).
FROM node:22-slim AS frontend-build

WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci

COPY frontend/ ./
# Same prepare steps `task frontend:build:proprietary` runs (env placeholder
# files, generated icon set, OG metadata), then the production build.
RUN npx tsx editor/scripts/setup-env.mts \
    && node editor/scripts/generate-icons.js \
    && node editor/scripts/generate-og-metadata.mjs \
    && npx vite build editor --mode proprietary

########################################################################
# Stage 3: PDFium (pinned revision, arch-correct, checksum-verified)
########################################################################
FROM debian:trixie-slim AS pdfium

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# The repo's own install script: resolves platform/arch from uname, downloads
# the pinned bblanchon/pdfium-binaries revision, and verifies its SHA-256.
# It writes to <script dir>/../.pdfium/current — /build/.pdfium/current here.
COPY rust/scripts/install-pdfium.sh /build/scripts/install-pdfium.sh
RUN bash /build/scripts/install-pdfium.sh

########################################################################
# Optional target: the AI engine sidecar (build with --target ai-engine)
########################################################################
FROM debian:trixie-slim AS ai-engine

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --user-group --create-home --shell /usr/sbin/nologin stirling

COPY --from=rust-build --chown=root:root /out/rustling-ai-engine /app/rustling-ai-engine

# The engine is stateless: the document/RAG store was removed with the PDF
# question-answer feature, so no /data volume is needed.
ENV RUSTLING_ENGINE_HOST=0.0.0.0 \
    RUSTLING_ENGINE_PORT=5001

# Stage-local redeclarations pick up the global defaults above (or the
# release workflow's --build-arg values). licenses matches the root LICENSE.
ARG VERSION
ARG REVISION
ARG CREATED
ARG SOURCE
LABEL org.opencontainers.image.title="RustlingPDF AI engine" \
      org.opencontainers.image.description="Optional AI-engine sidecar for RustlingPDF: typed AI operations on :5001" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.source="${SOURCE}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"

EXPOSE 5001/tcp
USER stirling
WORKDIR /app

HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=5 \
  CMD curl -fs --max-time 5 http://localhost:5001/health || exit 1

ENTRYPOINT ["tini", "--", "/app/rustling-ai-engine"]

########################################################################
# Final stage (default target): the RustlingPDF runtime image
########################################################################
FROM debian:trixie-slim AS runtime

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

# External tools, versus upstream Stirling-PDF's standard image (docker/base):
#   included — LibreOffice (writer/calc/impress/draw, -nogui variants),
#     qpdf (trixie's 12.2 satisfies the >=12 minimum),
#     Tesseract + eng + osd, OCRmyPDF (+unpaper/pngquant helpers), WeasyPrint
#     (trixie's 62.3 satisfies the >=58 minimum), poppler-utils (pdftohtml),
#     7zip (general archive tooling; its RAR *decompression* capability is
#     probed at startup — this DFSG build ships the RAR format handler, so it
#     can open a .rar and list/extract stored entries, but excludes the RAR
#     codecs, so it cannot decompress a RAR-compressed entry),
#     metric-compatible + broad-coverage fonts incl. CJK.
#   excluded — Calibre (~1 GB; ebook<->PDF endpoints self-disable with reason
#     DEPENDENCY), ImageMagick + Python venv extras + unoserver (Java-backend
#     needs; the Rust service calls soffice directly), unrar and 7zip-rar
#     (non-free, so CBR->PDF self-disables: this image's 7zip has no RAR
#     codec and no unrar/7zip-rar fallback, so the startup probe reports the
#     route dependency-disabled instead of shipping a decompressor-less
#     fallback), ffmpeg (PDF->video stays an explicit opt-in), veraPDF,
#     xvfb (the -nogui LibreOffice builds convert headless without X).
# Ghostscript is no longer requested: no code path in this service invokes `gs`
# (it was removed for its AGPL-3.0-or-commercial licence). It nevertheless still
# lands in the image, because `ocrmypdf` hard-Depends on it — measured on
# trixie-slim, the package set is byte-for-byte identical with and without the
# explicit entry (367 packages, ~1039 MiB installed). Dropping `ocrmypdf` too
# would take Ghostscript out and save ~81 MiB (320 packages, ~958 MiB), at the
# cost of the preferred OCR engine; that is a separate product decision.
# This sits between upstream's "standard" and "ultra-lite" variants: every
# common conversion tools, with graceful DEPENDENCY degradation for optional
# tools omitted by the image.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl tini fontconfig \
      libreoffice-writer-nogui libreoffice-calc-nogui \
      libreoffice-impress-nogui libreoffice-draw-nogui \
      qpdf poppler-utils \
      tesseract-ocr tesseract-ocr-eng tesseract-ocr-osd \
      ocrmypdf unpaper pngquant \
      weasyprint \
      7zip \
      fonts-dejavu fonts-liberation fonts-noto-core fonts-noto-cjk \
      fonts-crosextra-caladea fonts-crosextra-carlito \
    && rm -rf /var/lib/apt/lists/* \
    && fc-cache -f \
    && useradd --uid 1000 --user-group --create-home --shell /usr/sbin/nologin stirling \
    && mkdir -p /data/configs && chown -R stirling:stirling /data
# /data/configs exists only as a convenient mount point for operator-supplied
# configuration (see the RUSTLING_BASE_PATH comment below); the server never
# writes to it.

# Application binary, PDFium, and the baked-in SPA dist.
COPY --from=rust-build --chown=root:root /out/rustling-processing /app/rustling-processing
COPY --from=pdfium     --chown=root:root /build/.pdfium/current/  /app/pdfium/
COPY --from=frontend-build --chown=root:root /build/frontend/editor/dist/ /app/frontend/

# RUSTLING_BASE_PATH=/data: settings are read from /data/configs/settings.yml
# (and custom_settings.yml) when present, and customFiles overrides (shared
# signature assets, static SPA overrides, disclaimer texts) resolve beneath
# /data too. The server is stateless — it only ever READS from /data, so the
# whole tree can be bind-mounted read-only (e.g. -v ./data:/data:ro). With
# nothing mounted the image works out of the box: built-in template defaults
# plus SYSTEM_*/SECURITY_*/RUSTLING_* env overrides (legacy STIRLING_*
# aliases) apply. Job scratch space lives in the container's own tmp dir and
# is swept on a TTL, so no volume is declared.
ENV RUSTLING_HOST=0.0.0.0 \
    RUSTLING_PORT=8080 \
    RUSTLING_BASE_PATH=/data \
    RUSTLING_FRONTEND_DIST=/app/frontend \
    RUSTLING_PDFIUM_LIBRARY_PATH=/app/pdfium/libpdfium.so \
    HOME=/home/stirling

# Stage-local redeclarations pick up the global defaults above (or the
# release workflow's --build-arg values). licenses matches the root LICENSE.
ARG VERSION
ARG REVISION
ARG CREATED
ARG SOURCE
LABEL org.opencontainers.image.title="RustlingPDF" \
      org.opencontainers.image.description="Self-hosted PDF toolbox: pure-Rust backend + React SPA in one container" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.keywords="PDF, manipulation, API, Rust, axum, React" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.source="${SOURCE}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"

EXPOSE 8080/tcp
STOPSIGNAL SIGTERM
USER stirling
WORKDIR /app

HEALTHCHECK --interval=30s --timeout=15s --start-period=15s --retries=5 \
  CMD curl -fs --max-time 10 http://localhost:8080/api/v1/info/status || exit 1

ENTRYPOINT ["tini", "--", "/app/rustling-processing"]
