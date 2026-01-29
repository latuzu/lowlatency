#!/bin/bash
set -e

# Production data generation and validation script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

OUTPUT_FILE="${OUTPUT_FILE:-data.bin}"
RECORD_COUNT="${RECORD_COUNT:-100000000}"

echo "=== Low-Latency KV Store - Production Setup ==="
echo "Output file: $OUTPUT_FILE"
echo "Total records: $RECORD_COUNT"
echo ""

# Check if binaries exist
if [ ! -f "generate" ]; then
    echo "Building generate tool..."
    go build -o generate ./cmd/generate
fi

if [ ! -f "server" ]; then
    echo "Building server..."
    go build -o server main.go
fi

if [ ! -f "benchmark" ]; then
    echo "Building benchmark tool..."
    go build -o benchmark ./cmd/benchmark
fi

# Check available disk space
REQUIRED_SPACE=$((RECORD_COUNT * 320))
AVAILABLE_SPACE=$(df -B1 "$(dirname "$OUTPUT_FILE")" | tail -1 | awk '{print $4}')

echo "Required disk space: $(numfmt --to=iec-i --suffix=B $REQUIRED_SPACE)"
echo "Available disk space: $(numfmt --to=iec-i --suffix=B $AVAILABLE_SPACE)"

if [ "$AVAILABLE_SPACE" -lt "$REQUIRED_SPACE" ]; then
    echo "ERROR: Insufficient disk space!"
    exit 1
fi

# Generate data
if [ -f "$OUTPUT_FILE" ]; then
    echo "WARNING: $OUTPUT_FILE already exists."
    read -p "Overwrite? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Skipping data generation."
    else
        rm "$OUTPUT_FILE"
        echo "Generating $RECORD_COUNT records..."
        ./generate -count "$RECORD_COUNT" -output "$OUTPUT_FILE"
    fi
else
    echo "Generating $RECORD_COUNT records..."
    ./generate -count "$RECORD_COUNT" -output "$OUTPUT_FILE"
fi

echo ""
echo "=== Data Generation Complete ==="
ls -lh "$OUTPUT_FILE"
echo ""

# Validate data file
FILE_SIZE=$(stat -c%s "$OUTPUT_FILE" 2>/dev/null || stat -f%z "$OUTPUT_FILE" 2>/dev/null)
EXPECTED_SIZE=$((RECORD_COUNT * 320))

if [ "$FILE_SIZE" -eq "$EXPECTED_SIZE" ]; then
    echo "✓ Data file size is correct: $(numfmt --to=iec-i --suffix=B $FILE_SIZE)"
else
    echo "✗ ERROR: Data file size mismatch!"
    echo "  Expected: $(numfmt --to=iec-i --suffix=B $EXPECTED_SIZE)"
    echo "  Actual: $(numfmt --to=iec-i --suffix=B $FILE_SIZE)"
    exit 1
fi

echo ""
echo "=== Starting Validation Test ==="

# Start server in background
PORT=18080
./server -data "$OUTPUT_FILE" -port $PORT > /tmp/kv-server.log 2>&1 &
SERVER_PID=$!

# Wait for server to start
echo "Waiting for server to start (PID: $SERVER_PID)..."
sleep 5

# Check if server is running
if ! ps -p $SERVER_PID > /dev/null; then
    echo "ERROR: Server failed to start. Check /tmp/kv-server.log"
    cat /tmp/kv-server.log
    exit 1
fi

# Health check
echo "Running health check..."
HEALTH_RESPONSE=$(curl -s "http://localhost:$PORT/health")
echo "Health check response: $HEALTH_RESPONSE"

# Run quick benchmark
echo ""
echo "Running performance validation (10 seconds, 1000 QPS)..."
TEST_KEYS=$((RECORD_COUNT > 1000000 ? 1000000 : RECORD_COUNT))
./benchmark -url "http://localhost:$PORT" -qps 1000 -duration 10 -keys "$TEST_KEYS"

# Cleanup
echo ""
echo "Stopping server..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo ""
echo "=== Setup Complete ==="
echo "To start the production server, run:"
echo "  ./server -data $OUTPUT_FILE -port 8080"
