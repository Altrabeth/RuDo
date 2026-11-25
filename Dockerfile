# A minimal image of RuDo: the static musl binary, with the time zone database bundled, on
# `scratch`. Run it with `docker run -it --rm -v ~/todo:/workspace ghcr.io/altrabeth/rudo-tui`;
# docs/platforms.md describes the options and docs/RELEASING.md the release.

# The build stage runs on the platform of the build machine and cross-compiles for the target
# platform, which takes no emulation because RuDo has no C dependencies and rust-lld links musl
# binaries for every architecture. Keep the tag at the toolchain of rust-toolchain.toml.
FROM --platform=$BUILDPLATFORM rust:1.100.0-slim-trixie AS build
# Set by BuildKit from `--platform`, by default the platform of the build machine
ARG TARGETARCH
# The toolchain of the image instead of the one of rust-toolchain.toml, whose components the
# build does not need
ENV RUSTUP_TOOLCHAIN=1.100.0 \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
WORKDIR /src
COPY . .
RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) target=x86_64-unknown-linux-musl ;; \
        arm64) target=aarch64-unknown-linux-musl ;; \
        *) echo "error: unsupported architecture ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    rustup target add "$target"; \
    cargo build --locked --profile dist --target "$target"; \
    install -D "target/$target/dist/rudo" /rootfs/usr/local/bin/rudo; \
    install -d -m 1777 /rootfs/home/rudo /rootfs/workspace

FROM scratch
LABEL org.opencontainers.image.title="RuDo" \
      org.opencontainers.image.description="A keyboard-driven terminal UI for todo lists stored as Markdown files" \
      org.opencontainers.image.source="https://github.com/Altrabeth/RuDo" \
      org.opencontainers.image.url="https://github.com/Altrabeth/RuDo" \
      org.opencontainers.image.licenses="GPL-3.0-or-later"
# HOME, which holds the default configuration folder and the log file, and the workspace are
# writable by every user, since `--user` may choose any. Without a mounted folder, the workspace
# is lost with the container.
COPY --from=build /rootfs/ /
ENV HOME=/home/rudo
VOLUME ["/workspace"]
WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/rudo", "--root", "/workspace"]
