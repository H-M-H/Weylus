FROM debian:stretch
ENV RUSTUP_HOME="/usr/local/rustup" CARGO_HOME="/usr/local/cargo" PATH="/usr/local/cargo/bin:$PATH"
RUN apt-get update && \
   apt-get install -y libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev \
   libxrender-dev libxfixes-dev libgl1-mesa-dev libglu1-mesa-dev libxtst-dev cmake git curl \
   software-properties-common zip libssl-dev libxrandr-dev libxcomposite-dev libxi-dev \
   gcc g++ autoconf libtool-bin libxv-dev libdrm-dev libpango1.0-dev pkg-config \
   libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libdbus-1-dev mingw-w64
RUN apt-add-repository contrib
RUN apt-add-repository non-free
RUN apt-get update && apt-get install -y nvidia-cuda-dev
RUN curl -Lo cmake.tar.gz https://github.com/Kitware/CMake/releases/download/v3.23.3/cmake-3.23.3.tar.gz && tar xf cmake.tar.gz
RUN cd cmake-3.* && cmake . && make -j$(nproc) && make install
RUN rm -rf cmake*
RUN curl -LO "https://www.nasm.us/pub/nasm/releasebuilds/2.15.05/nasm-2.15.05.tar.xz" && \
    tar xf "nasm-2.15.05.tar.xz" && cd "nasm-2.15.05" && \
    ./configure --prefix=/usr && make -j$(nproc) && make install && cd .. && rm -rf "nasm-2.15.05*"
RUN curl -sL https://deb.nodesource.com/setup_16.x | bash - && \
    apt-get install -y nodejs && \
    npm install -g typescript
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain stable-x86_64-unknown-linux-gnu
RUN cargo install cargo-deb
RUN rustup target add x86_64-pc-windows-gnu
