FROM rust:1-bookworm

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        libayatana-appindicator3-dev \
        libgtk-3-dev \
        libmpv-dev \
        librsvg2-dev \
        libsoup-3.0-dev \
        libssl-dev \
        libwebkit2gtk-4.1-dev \
        nodejs \
        npm \
        openssh-client \
        patchelf \
        pkg-config \
    && npm install -g n \
    && n 24 \
    && npm install -g corepack@latest \
    && corepack enable \
    && corepack prepare pnpm@11.1.3 --activate \
    && rustup component add rustfmt clippy \
    && cargo install tauri-cli --locked \
    && rm -rf /var/lib/apt/lists/*
