FROM rust:alpine AS build

WORKDIR /caoq

RUN apk add clang lld libc-dev

COPY ./server/ ./server/
COPY ./core/ ./core/

WORKDIR /caoq/server

RUN cargo build --release

FROM alpine:latest

WORKDIR /caoq

COPY --from=build /caoq/server/target/release/caoq ./

ENTRYPOINT ["/caoq/caoq"]
