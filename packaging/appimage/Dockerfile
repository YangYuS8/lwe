FROM ubuntu:22.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        desktop-file-utils \
        file \
        git \
        libayatana-appindicator3-dev \
        libfuse2 \
        libgtk-3-dev \
        libmpv-dev \
        libpulse-dev \
        librsvg2-dev \
        libsoup-3.0-dev \
        libssl-dev \
        libwebkit2gtk-4.1-dev \
        patchelf \
        pkg-config \
        python3 \
        squashfs-tools \
        xdg-utils \
        xdotool \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*
