FROM rust:1.83.0 AS builder

# Set the working directory in the container
WORKDIR /app

COPY . .

# Build the application
RUN cargo build --release

FROM ubuntu:22.04

# RUN apt-get update && apt-get install -y libssl1.1 ca-certificates

WORKDIR /app

COPY --from=builder /app/target/release/monitoring-api .

EXPOSE 8080
EXPOSE 3001
EXPOSE 9090

# Set the command to run your app
CMD ["./monitoring-api"]

