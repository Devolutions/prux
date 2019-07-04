# ++++++++++++++++++++++++++++++++
# BUILD CONTAINER
# ++++++++++++++++++++++++++++++++

FROM rust:1.35-stretch as rust-build
LABEL maintainer "Devolutions Inc."

WORKDIR /rust/prux

COPY . .

RUN cargo build --release

# ++++++++++++++++++++++++++++++++
# PRODUCTION CONTAINER
# ++++++++++++++++++++++++++++++++

FROM debian:stretch-slim
LABEL maintainer "Devolutions Inc."

WORKDIR /etc/prux

# Copy Artifacts from Build Container
COPY --from=rust-build /rust/prux/target/release/prux .

EXPOSE 7479

ENTRYPOINT [ "./prux" ]