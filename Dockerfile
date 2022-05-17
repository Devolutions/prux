# ++++++++++++++++++++++++++++++++
# BUILD CONTAINER
# ++++++++++++++++++++++++++++++++

FROM rust:latest as rust-build
LABEL maintainer "Devolutions Inc."

WORKDIR /rust/prux

COPY . .

RUN cargo build --release

# ++++++++++++++++++++++++++++++++
# PRODUCTION CONTAINER
# ++++++++++++++++++++++++++++++++

FROM debian:stable-slim
LABEL maintainer "Devolutions Inc."

WORKDIR /etc/prux

RUN apt-get update
RUN apt-get install -y --no-install-recommends libssl1.1 ca-certificates libcurl4-openssl-dev
RUN rm -rf /var/lib/apt/lists/*

# Copy Artifacts from Build Container
COPY --from=rust-build /rust/prux/target/release/prux .

EXPOSE 7479

ENTRYPOINT [ "./prux" ]