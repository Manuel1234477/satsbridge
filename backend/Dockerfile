FROM rust:1.90-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 satsbridge && mkdir /data && chown satsbridge /data
COPY --from=build /app/target/release/satsbridge /usr/local/bin/satsbridge
USER satsbridge
ENV BIND_ADDR=0.0.0.0:8080 DATABASE_URL=sqlite:///data/satsbridge.db
EXPOSE 8080
CMD ["satsbridge"]
