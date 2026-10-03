FROM ubuntu:22.04@sha256:b1066385161d28ddf6bc7e7b28a9170eec11484c821d1a5150d176cbde41d7f7

ARG DEBIAN_FRONTEND=noninteractive
ARG FLUTTER_VERSION=3.47.5
ARG FLUTTER_REVISION=6a19cca56475dbfba1478ee68d7bd0c2ef891da1
ARG RUST_VERSION=1.98

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git unzip xz-utils zip python3 \
    clang cmake ninja-build pkg-config libgtk-3-dev libwayland-dev libstdc++-12-dev \
    build-essential perl libssl-dev file binutils dpkg-dev rpm \
    desktop-file-utils dbus dbus-x11 gnome-keyring xvfb xauth \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
ENV PATH=/opt/flutter/bin:/opt/cargo/bin:$PATH
RUN curl --fail --location --proto '=https' --tlsv1.2 \
      https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init \
      --output /tmp/rustup-init \
    && curl --fail --location --proto '=https' --tlsv1.2 \
      https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init.sha256 \
      --output /tmp/rustup-init.sha256 \
    && cd /tmp && sha256sum --check rustup-init.sha256 \
    && chmod 755 /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
    && rm /tmp/rustup-init /tmp/rustup-init.sha256

RUN git clone --depth 1 --branch "$FLUTTER_VERSION" https://github.com/flutter/flutter.git /opt/flutter \
    && test "$(git -C /opt/flutter rev-parse HEAD)" = "$FLUTTER_REVISION" \
    && flutter config --no-analytics --enable-linux-desktop \
    && flutter precache --linux \
    && flutter --version && rustc --version

# Flutter Impeller loads GLES at runtime rather than declaring an ELF NEEDED
# entry. Keep the same graphics/font runtime explicit in the isolated tests.
# The fixture's private D-Bus needs a FileChooser provider for native RDP folder
# selection. GTK is this isolated image's provider, not a user-desktop mandate.
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgles2 libegl1 libgl1-mesa-dri fonts-dejavu-core \
    xdg-desktop-portal xdg-desktop-portal-gtk \
    && rm -rf /var/lib/apt/lists/*

# Only a curated source snapshot is mounted at /work. Never mount a login's
# home, keyring, D-Bus socket, kubeconfig, SSH keys or private repository history.
WORKDIR /work
