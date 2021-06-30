FROM rust:latest as builder
WORKDIR /usr/src/tg
COPY . .
RUN if [ -e  tg.toml.override ]; then cp tg.toml.override tg.toml; fi 
RUN cargo install --path .


FROM debian:buster-slim
RUN apt-get update && apt-get install -y libssl1.1 libcairo-gobject2 ca-certificates
ARG TELEGRAM_BOT_TOKEN
RUN mkdir -p /tg
RUN echo "#!/bin/bash\nRUST_BACKTRACE=1 RUST_LOG=info TELEGRAM_BOT_TOKEN=${TELEGRAM_BOT_TOKEN} tg" > /tg/run.sh
RUN chmod +x /tg/run.sh
COPY --from=builder /usr/src/tg/tg.toml /tg

COPY --from=builder /usr/local/cargo/bin/tg /usr/local/bin/tg
WORKDIR /tg
CMD ["./run.sh"]
