#
# Copyright (c) 2026 Wing Mun Fung
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0, available at
# https://www.eclipse.org/legal/epl-2.0/, or the Apache License, Version 2.0,
# available at https://www.apache.org/licenses/LICENSE-2.0.
#
# SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
#

FROM rust:1.93-slim-bookworm AS rust
FROM ros:jazzy-ros-base
COPY --from=rust /usr/local/cargo /usr/local/cargo
COPY --from=rust /usr/local/rustup /usr/local/rustup
ENV PATH=/usr/local/cargo/bin:$PATH RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
RUN apt-get update && apt-get install -y --no-install-recommends \
    clang \
    libclang-dev \
    build-essential \
    pkg-config \
    libssl-dev \
    ros-jazzy-example-interfaces \
    ros-jazzy-test-msgs \
    ros-jazzy-rosapi \
    python3-pytest \
    python3-websockets \
    python3-cbor2 \
    python3-pil \
    && rm -rf /var/lib/apt/lists/*
RUN sed -i 's|http://ports.ubuntu.com|https://ports.ubuntu.com|g' \
        /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get -o Acquire::Retries=3 update \
    && apt-get -o Acquire::Retries=3 install -y --no-install-recommends \
    python3-matplotlib \
    python3-psutil \
    python3-autobahn \
    python3-twisted \
    ros-jazzy-rosbridge-server \
    ros-jazzy-launch-testing \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /work
CMD ["bash", "scripts/test.sh"]
