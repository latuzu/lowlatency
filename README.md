# Low-Latency Key-Value Store

A high-performance, low-latency key-value store designed for point lookups with guaranteed p99 latency ≤ 5ms.

## Performance Characteristics

- **Capacity:** 100,000,000 records
- **Key size:** 64 bytes (fixed-size)
- **Value size:** 256 bytes (fixed-size)
- **Throughput:** 10,000+ requests per second (sustained)
- **Query type:** Point lookup only (single key → single value)
- **Latency guarantee:** 99th percentile latency ≤ 5 milliseconds (p99 ≤ 5ms)

## Architecture

The system uses:
- **Memory-mapped storage:** Zero-copy reads directly from OS page cache
- **Hash-based indexing:** O(1) lookup time using Go's native map
- **Fixed-size records:** 320 bytes per record (64B key + 256B value)
- **HTTP server:** Lightweight, async Go HTTP server

## Quick Start

### Prerequisites

- Go 1.21 or higher
- Sufficient RAM for the index (approximately 3-4GB for 100M records)
- Sufficient disk space for data file (approximately 32GB for 100M records)

### Build

```bash
# Build the server
go build -o server main.go

# Build the data generator
go build -o generate ./cmd/generate

# Build the benchmark tool
go build -o benchmark ./cmd/benchmark
```

### Generate Test Data

```bash
# Generate 1 million records (for testing)
./generate -count 1000000 -output data.bin

# Generate 100 million records (production)
./generate -count 100000000 -output data.bin
```

This will create a binary file with sequential keys: `key-00000000000000000000`, `key-00000000000000000001`, etc.

### Start the Server

```bash
# Start with default settings (port 8080)
./server -data data.bin

# Specify custom port
./server -data data.bin -port 9090
```

### Query the API

```bash
# Health check
curl http://localhost:8080/health

# Get a value by key
curl "http://localhost:8080/get?key=key-00000000000000000000"

# Get another value
curl "http://localhost:8080/get?key=key-00000000000000000999"
```

### Run Benchmark

```bash
# Benchmark with 10,000 QPS for 10 seconds
./benchmark -url http://localhost:8080 -qps 10000 -duration 10 -keys 1000000

# Higher load test
./benchmark -url http://localhost:8080 -qps 20000 -duration 30 -keys 1000000
```

## API Reference

### GET /get

Retrieves a value by key.

**Parameters:**
- `key` (string, required): The key to lookup

**Response:**
- `200 OK`: Returns the value as binary data
- `404 Not Found`: Key does not exist
- `400 Bad Request`: Missing key parameter

**Example:**
```bash
curl "http://localhost:8080/get?key=key-00000000000000000000"
```

### GET /health

Health check endpoint.

**Response:**
- `200 OK`: Server is healthy, returns record count

**Example:**
```bash
curl http://localhost:8080/health
```

## Performance Optimization Tips

1. **Memory:** Ensure sufficient RAM for the index structure (approximately 100 bytes per key)
2. **File System:** Use a fast SSD or NVMe storage for the data file
3. **Network:** Deploy in the same data center / availability zone for low-latency networking
4. **OS Tuning:**
   - Increase file descriptor limits: `ulimit -n 65535`
   - Tune TCP settings for low latency
   - Consider huge pages for memory-mapped files

## File Format

The data file is a simple binary format:
- Each record is exactly 320 bytes
- First 64 bytes: Key (null-padded if key is shorter)
- Next 256 bytes: Value (null-padded if value is shorter)
- Records are sequential with no gaps

## Limitations

- Read-only: No support for writes, updates, or deletes after initial load
- Point lookups only: No range queries, scans, or complex queries
- Single-node: No built-in replication or distribution
- In-memory index: Requires sufficient RAM for hash table

## Scaling Considerations

For higher throughput or availability:
1. **Horizontal scaling:** Deploy multiple instances behind a load balancer
2. **Replication:** Replicate the data file to multiple servers
3. **Sharding:** Partition keys across multiple servers based on hash

## License

MIT
