FROM ubuntu:18.04
ENV RUSTUP_HOME="/usr/local/rustup" CARGO_HOME="/usr/local/cargo" PATH="/usr/local/cargo/bin:$PATH"
RUN apt-get update && \
        apt-get install -y libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev \
        libxrender-dev libxfixes-dev libgl1-mesa-dev libglu1-mesa-dev libxtst-dev cmake git curl \
        software-properties-common zip nasm libssl-dev
RUN curl -sL https://deb.nodesource.com/setup_14.x | bash - && \
    apt-get install -y nodejs && \
    npm install -g typescript
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain stable-x86_64-unknown-linux-gnu
RUN cargo install cargo-deb
