# Container environment for the Linux signed-upgrade e2e proof.
#
# Provides everything the flow needs on a host that only has Docker:
#   - Rust toolchain (base image) for the backend sidecar + src-tauri builds
#   - Tauri Linux system libraries (webkit2gtk 4.1 and friends)
#   - Xvfb + dbus so the desktop app can run headless
#   - Node.js 22 for the tauri CLI / vite build
#   - Task (taskfile.dev) because the staging scripts shell out to it
#   - Python 3 + websockets for the WebKit remote-inspector driver
#   - webkit2gtk-driver (WebKitWebDriver) as a fallback automation channel
#
# Built by run-e2e-container.sh; see that script for the bind mounts and
# environment the e2e run uses.
FROM rust:1.94-slim-trixie

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config build-essential libssl-dev \
    libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libgtk-3-dev \
    libsoup-3.0-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev \
    webkit2gtk-driver \
    xvfb xauth dbus dbus-x11 \
    libgl1 libgl1-mesa-dri libegl1 \
    fontconfig fonts-liberation \
    python3 python3-websockets \
    curl wget ca-certificates file git procps \
    && rm -rf /var/lib/apt/lists/*

# Node.js 22 (the tauri CLI is node-based; host node is too old / absent)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Task — the sidecar staging path is `task desktop:stage-sidecar`
RUN curl -sSL https://taskfile.dev/install.sh | sh -s -- -b /usr/local/bin

# AppImage bundling helpers: linuxdeploy validates the .desktop file,
# appimagetool can call zsyncmake for embedded update info, and the tauri
# bundler shells out to xdg-mime for the PDF file association.
RUN apt-get update && apt-get install -y --no-install-recommends \
    desktop-file-utils zsync xdg-utils shared-mime-info \
    && rm -rf /var/lib/apt/lists/*

# AppImage tooling (linuxdeploy/appimagetool, downloaded by the tauri CLI at
# bundle time) cannot FUSE-mount inside an unprivileged container; both honor
# this escape hatch. NO_STRIP avoids linuxdeploy's strip pass, which is flaky
# on large Rust binaries in containers.
ENV APPIMAGE_EXTRACT_AND_RUN=1 \
    NO_STRIP=1
