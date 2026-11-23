FROM rust:1-slim-bookworm AS build
WORKDIR /app
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd -m app
COPY --from=build /app/target/release/app-backend /usr/local/bin/reparto-backend
USER app
EXPOSE 8080
CMD ["app-backend"]
