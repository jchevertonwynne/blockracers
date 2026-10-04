# The lobby server's image. Nothing of the game goes into it.
#
# Built in three steps so that the slow part, compiling what the lobby depends on, is
# a layer of its own that only a change to a Cargo.toml or the lockfile undoes:
# cargo-chef notes what the workspace depends on, builds just that, and the lobby's
# own source is compiled on top.
FROM --platform=$BUILDPLATFORM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
WORKDIR /src
ARG TARGETARCH
# Built for the cluster's machine by Rust's own cross-compiling, not by emulating
# one. The musl targets carry their own C runtime and the lobby has no C in it, so
# the linker Rust ships is all the linking needs.
RUN case "$TARGETARCH" in \
      arm64) echo aarch64-unknown-linux-musl ;; \
      amd64) echo x86_64-unknown-linux-musl ;; \
      *) echo "no target for $TARGETARCH" >&2; exit 1 ;; \
    esac > /target && rustup target add "$(cat /target)"
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld

# The whole workspace goes in here, since its one lockfile is the game's too, but
# all that comes out is the recipe: the manifests and the lockfile.
FROM chef AS plan
COPY . .
RUN cargo chef prepare --recipe-path /recipe.json

FROM chef AS build
COPY --from=plan /recipe.json /recipe.json
# Only the lobby and what it depends on are fetched and built.
RUN cargo chef cook --locked --release -p lobby --target "$(cat /target)" --recipe-path /recipe.json
COPY . .
RUN cargo build --locked --release -p lobby --target "$(cat /target)" \
    && cp "target/$(cat /target)/release/lobby" /lobby

# scratch: a static binary that makes no outbound calls needs nothing else.
FROM scratch
COPY --from=build /lobby /lobby
USER 65532:65532
ENTRYPOINT ["/lobby"]
CMD ["-addr", ":8096"]
