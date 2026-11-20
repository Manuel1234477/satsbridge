FROM rust:1.90-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 stellarbridge && mkdir /data && chown stellarbridge /data
COPY --from=build /app/target/release/stellarbridge /usr/local/bin/stellarbridge
USER stellarbridge
ENV BIND_ADDR=0.0.0.0:8080 DATABASE_URL=sqlite:///data/stellarbridge.db
EXPOSE 8080
CMD ["stellarbridge"]
