FROM debian:bullseye
ENV RUSTUP_HOME="/usr/local/rustup" CARGO_HOME="/usr/local/cargo" PATH="/usr/local/cargo/bin:$PATH"
RUN apt-get update && \
   apt-get install -y libgl1-mesa-dev libglu1-mesa-dev cmake git curl \
   software-properties-common zip libssl-dev \
   gcc g++ autoconf libtool-bin libdrm-dev pkg-config mingw-w64 \
   libpipewire-0.3-dev libdbus-1-dev clang \
   libwayland-dev libxkbcommon-dev
RUN apt-add-repository contrib
RUN apt-add-repository non-free
RUN apt-get update && apt-get install -y nvidia-cuda-dev
RUN curl -Lo cmake.tar.gz https://github.com/Kitware/CMake/releases/download/v4.1.1/cmake-4.1.1.tar.gz && tar xf cmake.tar.gz
RUN cd cmake-4.* && cmake . && make -j$(nproc) && make install
RUN rm -rf cmake*
RUN curl -LO "https://www.nasm.us/pub/nasm/releasebuilds/2.16.03/nasm-2.16.03.tar.xz" && \
    tar xf "nasm-2.16.03.tar.xz" && cd "nasm-2.16.03" && \
    ./configure --prefix=/usr && make -j$(nproc) && make install && cd .. && rm -rf "nasm-2.16.03*"
RUN curl -sL https://deb.nodesource.com/setup_22.x | bash - && \
    apt-get install -y nodejs && \
    npm install -g typescript
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain stable-x86_64-unknown-linux-gnu
RUN cargo install cargo-deb
RUN rustup target add x86_64-pc-windows-gnu
