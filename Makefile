.PHONY: all build clean test benchmark generate-data run help

# Build configuration
BINARY_SERVER=server
BINARY_GENERATE=generate
BINARY_BENCHMARK=benchmark

# Default data settings
DATA_FILE=data.bin
RECORD_COUNT=1000000
PORT=8080

all: build

help:
	@echo "Low-Latency Key-Value Store - Makefile targets:"
	@echo ""
	@echo "  make build           - Build all binaries"
	@echo "  make clean           - Remove binaries and data files"
	@echo "  make generate-data   - Generate test data (1M records by default)"
	@echo "  make run             - Start the server"
	@echo "  make benchmark       - Run benchmark tests"
	@echo "  make test            - Generate data, run server, and benchmark"
	@echo ""
	@echo "Configuration (override with make VAR=value):"
	@echo "  RECORD_COUNT=${RECORD_COUNT}  - Number of records to generate"
	@echo "  DATA_FILE=${DATA_FILE}        - Data file path"
	@echo "  PORT=${PORT}                  - Server port"

build:
	@echo "Building binaries..."
	@go build -o $(BINARY_SERVER) main.go
	@go build -o $(BINARY_GENERATE) ./cmd/generate
	@go build -o $(BINARY_BENCHMARK) ./cmd/benchmark
	@echo "✓ Build complete"

clean:
	@echo "Cleaning up..."
	@rm -f $(BINARY_SERVER) $(BINARY_GENERATE) $(BINARY_BENCHMARK)
	@rm -f *.bin
	@echo "✓ Clean complete"

generate-data: build
	@echo "Generating $(RECORD_COUNT) records..."
	@./$(BINARY_GENERATE) -count $(RECORD_COUNT) -output $(DATA_FILE)

run: build
	@echo "Starting server on port $(PORT)..."
	@./$(BINARY_SERVER) -data $(DATA_FILE) -port $(PORT)

benchmark: build
	@echo "Running benchmark..."
	@./$(BINARY_BENCHMARK) -url http://localhost:$(PORT) -qps 10000 -duration 10 -keys $(RECORD_COUNT)

test: clean generate-data
	@echo "Starting server in background..."
	@./$(BINARY_SERVER) -data $(DATA_FILE) -port $(PORT) > server.log 2>&1 & echo $$! > server.pid
	@sleep 2
	@echo "Running health check..."
	@curl -s http://localhost:$(PORT)/health
	@echo ""
	@echo "Running benchmark..."
	@./$(BINARY_BENCHMARK) -url http://localhost:$(PORT) -qps 1000 -duration 5 -keys $(RECORD_COUNT)
	@echo "Stopping server..."
	@kill `cat server.pid` && rm -f server.pid server.log
	@echo "✓ Test complete"
