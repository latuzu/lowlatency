# Implementation Plan

## Project Structure

```
lowlatency/
├── Cargo.toml                    # Workspace definition
├── crates/
│   ├── kv-server/                # Main gRPC server binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs           # Entry point
│   │       ├── server.rs         # gRPC service implementation
│   │       ├── config.rs         # Configuration loading
│   │       └── metrics.rs        # Prometheus metrics
│   │
│   ├── kv-store/                 # Core storage library
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            # Public API
│   │       ├── store.rs          # KvStore implementation
│   │       ├── mphf.rs           # MPHF wrapper
│   │       ├── mmap.rs           # Memory mapping utilities
│   │       └── record.rs         # Record layout (key + value)
│   │
│   ├── snapshot-builder/         # Offline snapshot generation tool
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs           # CLI entry point
│   │       ├── builder.rs        # Snapshot construction
│   │       └── source.rs         # Data source readers
│   │
│   └── validator/                # Pre-deployment validation tool
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           # CLI entry point
│           └── validator.rs      # Validation logic
│
├── proto/
│   └── kv.proto                  # gRPC service definition
│
├── deploy/
│   ├── terraform/                # AWS infrastructure
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   ├── outputs.tf
│   │   ├── ec2.tf
│   │   ├── nlb.tf
│   │   └── s3.tf
│   │
│   └── scripts/
│       ├── deploy-snapshot.sh    # Upload snapshot to S3
│       ├── warmup-node.sh        # Pre-load data into memory
│       ├── cutover.sh            # Blue/Green switch
│       └── rollback.sh           # Emergency rollback
│
├── test/
│   ├── load/                     # Load testing scripts
│   │   └── benchmark.sh
│   └── data/                     # Test data generators
│       └── generate-test-data.rs
│
└── docs/
    ├── architecture.md           # System architecture
    └── implementation-plan.md    # This file
```

---

## Phase 1: Core Storage Library (`kv-store`)

### 1.1 Define Record Layout

**File:** `crates/kv-store/src/record.rs`

```rust
/// Fixed-size record: 64-byte key + 256-byte value = 320 bytes
#[repr(C, packed)]
pub struct Record {
    key: [u8; 64],
    value: [u8; 256],
}

impl Record {
    pub const SIZE: usize = 320;

    pub fn key(&self) -> &[u8; 64] {
        &self.key
    }

    pub fn value(&self) -> &[u8; 256] {
        &self.value
    }
}
```

### 1.2 Implement MPHF Wrapper

**File:** `crates/kv-store/src/mphf.rs`

```rust
use ph::fmph::Function;

pub struct Mphf {
    function: Function,
}

impl Mphf {
    pub fn build(keys: &[[u8; 64]]) -> Self {
        let function = Function::from(keys.iter().map(|k| k.as_slice()));
        Self { function }
    }

    pub fn get(&self, key: &[u8; 64]) -> Option<usize> {
        self.function.get(key.as_slice())
    }

    pub fn serialize(&self) -> Vec<u8> { /* ... */ }
    pub fn deserialize(data: &[u8]) -> Self { /* ... */ }
}
```

### 1.3 Implement Memory-Mapped Store

**File:** `crates/kv-store/src/store.rs`

```rust
use memmap2::Mmap;
use std::sync::Arc;

pub struct KvStore {
    mmap: Mmap,
    mphf: Mphf,
    record_count: usize,
    data_offset: usize,  // Offset to record array in mmap
}

impl KvStore {
    /// Load store from snapshot file
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        // Parse header
        let header = Header::from_bytes(&mmap[0..Header::SIZE]);
        header.validate()?;

        // Load MPHF
        let mphf_end = Header::SIZE + header.mphf_size;
        let mphf = Mphf::deserialize(&mmap[Header::SIZE..mphf_end]);

        Ok(Self {
            mmap,
            mphf,
            record_count: header.record_count,
            data_offset: align_to_4k(mphf_end),
        })
    }

    /// O(1) lookup
    #[inline]
    pub fn get(&self, key: &[u8; 64]) -> Option<&[u8; 256]> {
        let slot = self.mphf.get(key)?;
        if slot >= self.record_count {
            return None;
        }

        let record = self.get_record(slot);
        if record.key() == key {
            Some(record.value())
        } else {
            None
        }
    }

    #[inline]
    fn get_record(&self, slot: usize) -> &Record {
        let offset = self.data_offset + slot * Record::SIZE;
        unsafe {
            &*(self.mmap[offset..].as_ptr() as *const Record)
        }
    }
}
```

### 1.4 Tasks for Phase 1

- [ ] Create workspace `Cargo.toml`
- [ ] Implement `Record` struct with proper alignment
- [ ] Implement `Mphf` wrapper around `ph` crate
- [ ] Implement `Header` struct with magic number, checksum validation
- [ ] Implement `KvStore` with `open()` and `get()`
- [ ] Add unit tests for lookup correctness
- [ ] Benchmark single-threaded lookup performance (target: < 1μs)

---

## Phase 2: Snapshot Builder (`snapshot-builder`)

### 2.1 Snapshot File Format

```
Offset      Size        Content
─────────────────────────────────────────────
0x0000      4096        Header (4KB aligned)
0x1000      ~30MB       MPHF index (4KB aligned)
~0x1E00000  32GB        Record array (4KB aligned)
```

### 2.2 Builder Implementation

**File:** `crates/snapshot-builder/src/builder.rs`

```rust
pub struct SnapshotBuilder {
    output_path: PathBuf,
    keys: Vec<[u8; 64]>,
    values: Vec<[u8; 256]>,
}

impl SnapshotBuilder {
    pub fn new(output_path: PathBuf) -> Self { /* ... */ }

    pub fn add_record(&mut self, key: [u8; 64], value: [u8; 256]) {
        self.keys.push(key);
        self.values.push(value);
    }

    pub fn build(self) -> Result<(), Error> {
        // 1. Build MPHF from keys
        let mphf = Mphf::build(&self.keys);

        // 2. Create output file
        let file = File::create(&self.output_path)?;
        file.set_len(self.calculate_file_size())?;
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        // 3. Write header
        let header = Header::new(self.keys.len(), /* ... */);
        mmap[0..Header::SIZE].copy_from_slice(&header.to_bytes());

        // 4. Write MPHF
        let mphf_bytes = mphf.serialize();
        mmap[Header::SIZE..Header::SIZE + mphf_bytes.len()]
            .copy_from_slice(&mphf_bytes);

        // 5. Write records in MPHF order
        let data_offset = self.calculate_data_offset();
        for (i, (key, value)) in self.keys.iter().zip(&self.values).enumerate() {
            let slot = mphf.get(key).unwrap();
            let record_offset = data_offset + slot * Record::SIZE;
            mmap[record_offset..record_offset + 64].copy_from_slice(key);
            mmap[record_offset + 64..record_offset + 320].copy_from_slice(value);
        }

        // 6. Compute and write checksum
        let checksum = sha256(&mmap[Header::CHECKSUM_OFFSET..]);
        mmap[Header::CHECKSUM_RANGE].copy_from_slice(&checksum);

        // 7. Flush to disk
        mmap.flush()?;
        Ok(())
    }
}
```

### 2.3 CLI Interface

```bash
# Build snapshot from CSV
snapshot-builder \
  --input data.csv \
  --key-column 0 \
  --value-column 1 \
  --output snapshot.bin

# Build from custom binary format
snapshot-builder \
  --input data.bin \
  --format binary \
  --output snapshot.bin
```

### 2.4 Tasks for Phase 2

- [ ] Implement `SnapshotBuilder` struct
- [ ] Implement CSV reader for input data
- [ ] Implement binary format reader
- [ ] Add progress reporting for large datasets
- [ ] Compute and verify SHA256 checksum
- [ ] Test with 100M records (ensure completes in < 1 hour)
- [ ] Add S3 upload functionality

---

## Phase 3: gRPC Server (`kv-server`)

### 3.1 Protocol Definition

**File:** `proto/kv.proto`

```protobuf
syntax = "proto3";

package kv;

service KVService {
  rpc Get(GetRequest) returns (GetResponse);
  rpc BatchGet(BatchGetRequest) returns (BatchGetResponse);
  rpc Health(HealthRequest) returns (HealthResponse);
}

message GetRequest {
  bytes key = 1;  // Must be exactly 64 bytes
}

message GetResponse {
  bool found = 1;
  bytes value = 2;  // 256 bytes if found
}

message BatchGetRequest {
  repeated bytes keys = 1;
}

message BatchGetResponse {
  repeated GetResponse results = 1;
}

message HealthRequest {}

message HealthResponse {
  bool healthy = 1;
  string version = 2;
  uint64 record_count = 3;
}
```

### 3.2 Server Implementation

**File:** `crates/kv-server/src/server.rs`

```rust
use arc_swap::ArcSwap;
use std::sync::Arc;
use tonic::{Request, Response, Status};

pub struct KvServiceImpl {
    store: Arc<ArcSwap<KvStore>>,
    metrics: Metrics,
}

#[tonic::async_trait]
impl KvService for KvServiceImpl {
    async fn get(
        &self,
        request: Request<GetRequest>,
    ) -> Result<Response<GetResponse>, Status> {
        let start = Instant::now();

        let key = request.into_inner().key;
        if key.len() != 64 {
            return Err(Status::invalid_argument("key must be 64 bytes"));
        }

        let key: &[u8; 64] = key.as_slice().try_into().unwrap();
        let store = self.store.load();

        let response = match store.get(key) {
            Some(value) => GetResponse {
                found: true,
                value: value.to_vec(),
            },
            None => GetResponse {
                found: false,
                value: vec![],
            },
        };

        self.metrics.record_latency(start.elapsed());
        self.metrics.increment_requests();

        Ok(Response::new(response))
    }
}
```

### 3.3 Metrics

**File:** `crates/kv-server/src/metrics.rs`

```rust
use prometheus::{Histogram, IntCounter, Registry};

pub struct Metrics {
    pub request_latency: Histogram,
    pub requests_total: IntCounter,
    pub errors_total: IntCounter,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        let request_latency = Histogram::with_opts(
            HistogramOpts::new("request_latency_seconds", "Request latency")
                .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1]),
        ).unwrap();

        // ... register metrics

        Self { request_latency, requests_total, errors_total }
    }
}
```

### 3.4 Tasks for Phase 3

- [ ] Define protobuf schema
- [ ] Generate Rust code with `tonic-build`
- [ ] Implement `KvServiceImpl`
- [ ] Add Prometheus metrics endpoint (`/metrics`)
- [ ] Add health check endpoint
- [ ] Implement graceful shutdown
- [ ] Add hot-reload support via `ArcSwap`
- [ ] Benchmark with `ghz` (target: > 50k RPS single node)

---

## Phase 4: Deployment Infrastructure

### 4.1 Terraform Resources

**File:** `deploy/terraform/main.tf`

```hcl
provider "aws" {
  region = var.aws_region
}

# VPC and networking
module "vpc" {
  source = "terraform-aws-modules/vpc/aws"
  name   = "kv-vpc"
  cidr   = "10.0.0.0/16"

  azs             = [var.availability_zone]
  private_subnets = ["10.0.1.0/24"]
}

# Placement group for low-latency
resource "aws_placement_group" "kv" {
  name     = "kv-cluster"
  strategy = "cluster"
}

# EC2 instances
resource "aws_instance" "kv_blue" {
  count                = 3
  ami                  = data.aws_ami.amazon_linux_2023.id
  instance_type        = "r6i.4xlarge"
  placement_group      = aws_placement_group.kv.id
  subnet_id            = module.vpc.private_subnets[0]
  iam_instance_profile = aws_iam_instance_profile.kv.name

  root_block_device {
    volume_type = "gp3"
    volume_size = 100
    iops        = 3000
    encrypted   = true
  }

  tags = { Name = "kv-blue-${count.index}", Pool = "blue" }
}

resource "aws_instance" "kv_green" {
  count         = 3
  # Same as blue...
  tags = { Name = "kv-green-${count.index}", Pool = "green" }
}

# NLB
resource "aws_lb" "kv" {
  name               = "kv-nlb"
  internal           = true
  load_balancer_type = "network"
  subnets            = module.vpc.private_subnets
}

# Target groups
resource "aws_lb_target_group" "blue" {
  name     = "kv-blue"
  port     = 9090
  protocol = "TCP"
  vpc_id   = module.vpc.vpc_id

  health_check {
    protocol = "TCP"
    port     = "9091"
  }
}

resource "aws_lb_target_group" "green" {
  name     = "kv-green"
  port     = 9090
  protocol = "TCP"
  vpc_id   = module.vpc.vpc_id

  health_check {
    protocol = "TCP"
    port     = "9091"
  }
}

# Listener with weighted routing
resource "aws_lb_listener" "kv" {
  load_balancer_arn = aws_lb.kv.arn
  port              = 9090
  protocol          = "TCP"

  default_action {
    type = "forward"
    forward {
      target_group { arn = aws_lb_target_group.blue.arn; weight = 1 }
      target_group { arn = aws_lb_target_group.green.arn; weight = 0 }
    }
  }
}
```

### 4.2 Tasks for Phase 4

- [ ] Create Terraform modules for VPC, EC2, NLB, S3
- [ ] Create IAM roles for S3 access
- [ ] Create systemd service file for kv-server
- [ ] Create deployment scripts (cutover.sh, rollback.sh)
- [ ] Test blue/green switching
- [ ] Document runbook procedures

---

## Phase 5: Validation & Testing

### 5.1 Validator Tool

```bash
# Validate snapshot integrity
validator \
  --snapshot /data/snapshot.bin \
  --sample-size 10000

# Validate against expected results
validator \
  --snapshot /data/snapshot.bin \
  --queries test-keys.txt \
  --expected test-values.txt
```

### 5.2 Load Testing

```bash
# Sustained load test
ghz --insecure \
  --proto proto/kv.proto \
  --call kv.KVService/Get \
  --data-file test-keys.json \
  --qps 10000 \
  --duration 300s \
  --connections 50 \
  localhost:9090

# Verify results
# - p99 latency < 5ms
# - Error rate = 0%
# - QPS sustained at 10,000
```

### 5.3 Tasks for Phase 5

- [ ] Implement validator CLI
- [ ] Create test data generator (100M random records)
- [ ] Run load tests locally
- [ ] Run load tests on AWS
- [ ] Test node failure scenarios
- [ ] Test blue/green cutover under load
- [ ] Document performance results

---

## Implementation Order

1. **Week 1:** Phase 1 (kv-store library) + Phase 2 (snapshot-builder)
2. **Week 2:** Phase 3 (gRPC server) + local benchmarking
3. **Week 3:** Phase 4 (Terraform) + Phase 5 (validation)
4. **Week 4:** Integration testing + documentation

---

## Dependencies (Cargo.toml)

```toml
[workspace]
members = ["crates/*"]
resolver = "2"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
tonic = "0.12"
prost = "0.13"
memmap2 = "0.9"
ph = "0.8"
arc-swap = "1.7"
prometheus = "0.13"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
sha2 = "0.10"
bytes = "1"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "1"

[profile.release]
lto = true
codegen-units = 1
panic = "abort"
```

---

## Success Criteria

| Metric | Target |
|--------|--------|
| p99 latency | < 5ms |
| Throughput (single node) | > 50,000 RPS |
| Throughput (3 nodes) | > 100,000 RPS |
| Snapshot build time (100M) | < 1 hour |
| Blue/green cutover time | < 1 second |
| Rollback time | < 30 seconds |
| Memory usage per node | < 80 GB |
