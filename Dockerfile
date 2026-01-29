# Build stage
FROM golang:1.21-alpine AS builder

WORKDIR /build

# Copy go mod files
COPY go.mod ./

# Copy source code
COPY main.go ./
COPY cmd/ ./cmd/

# Build binaries
RUN go build -o server main.go && \
    go build -o generate ./cmd/generate && \
    go build -o benchmark ./cmd/benchmark

# Runtime stage
FROM alpine:latest

RUN apk --no-cache add ca-certificates curl

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /build/server /app/
COPY --from=builder /build/generate /app/
COPY --from=builder /build/benchmark /app/

# Expose port
EXPOSE 8080

# Run server
CMD ["./server", "-data", "/data/data.bin", "-port", "8080"]
