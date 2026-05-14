# OpenRD dev image.
#
# Builds and tests the workspace inside a Debian-based Rust container.
# The server is Linux-only (PipeWire / X11 / uinput targets), so this
# is also the canonical dev environment.

FROM rust:1-bookworm AS dev

# Tools used by future capture/encode work. Kept minimal for now;
# add more (x264, pipewire-dev, etc.) when those paths land.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential \
      pkg-config \
      ca-certificates \
      curl \
 && rm -rf /var/lib/apt/lists/*

# rustfmt + clippy for hygiene.
RUN rustup component add rustfmt clippy

WORKDIR /work

# Default to an interactive shell; compose overrides for build/test/run.
CMD ["bash"]
