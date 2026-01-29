# Low-Latency Key-Value Lookup System Architecture

## Executive Summary

Design a production system for 100M key-value lookups with p99 ≤ 5ms latency at 10k RPS, featuring RAM-resident serving and blue/green T+1 data deployment.

**Tech Stack:** Rust + AWS EC2 + Full Replication

---

## 1. Architecture Overview

```
                                    ┌─────────────────────────────────────────┐
                                    │           Data Pipeline (Offline)        │
                                    │  ┌─────────┐    ┌──────────┐            │
                                    │  │ Source  │───▶│ Snapshot │            │
                                    │  │  Data   │    │ Builder  │            │
                                    │  └─────────┘    └────┬─────┘            │
                                    └──────────────────────┼──────────────────┘
                                                           │
                                                           ▼
                                    ┌─────────────────────────────────────────┐
                                    │        Object Storage (S3)              │
                                    │   snapshots/v{N}/data.bin               │
                                    │   snapshots/v{N}/index.mph              │
                                    └────────────────────┬────────────────────┘
                                                         │
                         ┌───────────────────────────────┼───────────────────────────────┐
                         │                               │                               │
                         ▼                               ▼                               ▼
              ┌──────────────────┐            ┌──────────────────┐            ┌──────────────────┐
              │   Node 1 (Blue)  │            │   Node 2 (Blue)  │            │   Node 3 (Blue)  │
              │   Version V      │            │   Version V      │            │   Version V      │
              │   ┌───────────┐  │            │   ┌───────────┐  │            │   ┌───────────┐  │
              │   │ MPHF Index│  │            │   │ MPHF Index│  │            │   │ MPHF Index│  │
              │   │ + Data    │  │            │   │ + Data    │  │            │   │ + Data    │  │
              │   │ (mmap)    │  │            │   │ (mmap)    │  │            │   │ (mmap)    │  │
              │   └───────────┘  │            │   └───────────┘  │            │   └───────────┘  │
              └────────┬─────────┘            └────────┬─────────┘            └────────┬─────────┘
                       │                               │                               │
              ┌──────────────────┐            ┌──────────────────┐            ┌──────────────────┐
              │  Node 1 (Green)  │            │  Node 2 (Green)  │            │  Node 3 (Green)  │
              │   Version V+1    │            │   Version V+1    │            │   Version V+1    │
              │   (standby)      │            │   (standby)      │            │   (standby)      │
              └──────────────────┘            └──────────────────┘            └──────────────────┘
                       │                               │                               │
                       └───────────────────────────────┼───────────────────────────────┘
                                                       │
                                                       ▼
                                    ┌─────────────────────────────────────────┐
                                    │         AWS NLB (TCP L4)                │
                                    │    - Health checks (TCP 9091)           │
                                    │    - Target group weighted routing      │
                                    │    - Blue/Green pool switching          │
                                    └────────────────────┬────────────────────┘
                                                         │
                                                         ▼
                                    ┌─────────────────────────────────────────┐
                                    │              Client Pool                │
                                    │    - Connection pooling (HTTP/2)        │
                                    │    - Client-side load balancing         │
                                    └─────────────────────────────────────────┘
```

### Why Full Replication over Sharding

- **Simpler routing:** Any node can serve any request (no consistent hashing)
- **Better fault tolerance:** Node failure doesn't create "cold spots"
- **Easier blue/green:** Switch entire cluster atomically
- **RAM cost is manageable:** 32GB raw data fits comfortably in modern servers
- **10k RPS is trivial:** ~3,333 RPS per node with 3 nodes (15x headroom)

---

## 2. Data Format + Indexing Strategy

### 2.1 Minimal Perfect Hash Function (MPHF)

**What is MPHF:** A hash function that maps N keys to exactly N consecutive integers [0, N-1] with no collisions. Enables O(1) lookup with minimal space overhead.

**Implementation:** CHD (Compress, Hash, Displace) algorithm via `ph` crate
- Space: ~2.5 bits per key = **~30 MB** for 100M keys
- Lookup: 2-3 hash computations + 1 memory access

### 2.2 Memory Layout (Single Contiguous File)

```
┌─────────────────────────────────────────────────────────────────┐
│                        SNAPSHOT FILE                            │
├─────────────────────────────────────────────────────────────────┤
│ Header (4KB aligned)                                            │
│   - Magic number (8 bytes): 0x4B56535331303000 ("KVSS100\0")   │
│   - Version (8 bytes)                                           │
│   - Record count (8 bytes)                                      │
│   - Checksum (32 bytes - SHA256)                                │
│   - Build timestamp (8 bytes)                                   │
│   - Reserved (padding to 4KB)                                   │
├─────────────────────────────────────────────────────────────────┤
│ MPHF Index (~30 MB, 4KB aligned)                                │
│   - Serialized CHD structure                                    │
├─────────────────────────────────────────────────────────────────┤
│ Key-Value Data Array (32 GB, 4KB aligned)                       │
│   Record[0]: [Key: 64B][Value: 256B] = 320 bytes                │
│   Record[1]: [Key: 64B][Value: 256B] = 320 bytes                │
│   ...                                                           │
│   Record[99,999,999]: [Key: 64B][Value: 256B]                   │
└─────────────────────────────────────────────────────────────────┘
```

**Total file size:** ~32.03 GB

### 2.3 Lookup Algorithm

```rust
fn lookup(key: &[u8; 64]) -> Option<&[u8; 256]> {
    let slot = mphf.get(key)?;           // O(1), ~100ns
    let record = data_array[slot];       // Single memory access, ~50ns
    if record.key() == key {             // Verify key match
        Some(record.value())
    } else {
        None                             // Unknown key
    }
}
```

**Why store keys:** MPHF guarantees no collisions for the known keyset, but queries with unknown keys could map to any slot. We must verify.

---

## 3. Request Path and Latency Budget

### 3.1 Latency Measurement Definition

**What we measure:** Round-trip time from client sending request to receiving complete response.

```
Client                    NLB                    Server
  │                       │                        │
  ├──── t0: Send ────────▶│                        │
  │                       ├──── t1: Forward ──────▶│
  │                       │                        ├── t2: Process
  │                       │                        │   - Deserialize
  │                       │                        │   - MPHF lookup
  │                       │                        │   - Serialize
  │                       │◀──── t3: Response ─────┤
  │◀──── t4: Receive ─────┤                        │
  │                       │                        │

  Measured latency = t4 - t0
```

### 3.2 Latency Budget (p99 target: 5ms)

| Component | p99 Budget | Notes |
|-----------|------------|-------|
| Client serialization | 0.1 ms | Protobuf encode 64-byte key |
| Network: Client → NLB | 0.2 ms | Same AZ, ~0.1ms RTT typical |
| NLB processing | 0.1 ms | L4 passthrough, minimal |
| Network: NLB → Server | 0.2 ms | Same AZ |
| Server processing | 0.3 ms | Deserialize + lookup + serialize |
| Network: Server → NLB → Client | 0.4 ms | Return path |
| **Buffer for variance** | **3.7 ms** | Scheduling, queueing |
| **TOTAL** | **5.0 ms** | |

---

## 4. Capacity Sizing

### 4.1 Memory Requirements

| Component | Size | Notes |
|-----------|------|-------|
| Raw data (keys + values) | 32.00 GB | 100M × 320 bytes |
| MPHF index | 0.03 GB | ~2.5 bits/key |
| Memory-mapped overhead | 0.50 GB | Page tables, metadata |
| OS + runtime | 2.00 GB | Network buffers |
| Blue/Green buffer | 34.50 GB | Second dataset during transition |
| **Safety margin (20%)** | **6.90 GB** | |
| **TOTAL per node** | **~76 GB** | Recommend **96 GB RAM** |

### 4.2 AWS Instance Selection

| Instance | vCPU | RAM | Network | Use Case |
|----------|------|-----|---------|----------|
| `r6i.2xlarge` | 8 | 64 GB | 12.5 Gbps | Minimal (no B/G overlap) |
| `r6i.4xlarge` | 16 | 128 GB | 12.5 Gbps | **Recommended** (B/G overlap) |

### 4.3 Node Count

| Metric | Value |
|--------|-------|
| Target RPS | 10,000 |
| RPS per node (conservative) | 50,000+ |
| Nodes for HA | 3 (N+1 redundancy) |
| RPS per node with 3 nodes | 3,333 (15x headroom) |

---

## 5. Blue/Green T+1 Deployment

### 5.1 Timeline

```
T-4h    T-3h    T-2h    T-1h    T (cutover)    T+1h
  │       │       │       │          │           │
  ▼       ▼       ▼       ▼          ▼           ▼
Build   Upload  Download Warmup   Switch      Cleanup
Snapshot to S3   to Green  Green   Traffic     Blue
```

### 5.2 AWS NLB Blue/Green Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         AWS NLB                                 │
│  ┌─────────────────────┐    ┌─────────────────────┐            │
│  │  Target Group BLUE  │    │  Target Group GREEN │            │
│  │  (active, weight=1) │    │  (standby, weight=0)│            │
│  └──────────┬──────────┘    └──────────┬──────────┘            │
└─────────────┼───────────────────────────┼──────────────────────┘
              │                           │
    ┌─────────┼─────────┐       ┌─────────┼─────────┐
    ▼         ▼         ▼       ▼         ▼         ▼
┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐
│Node 1 │ │Node 2 │ │Node 3 │ │Node 1 │ │Node 2 │ │Node 3 │
│Blue V │ │Blue V │ │Blue V │ │Green  │ │Green  │ │Green  │
│       │ │       │ │       │ │V+1    │ │V+1    │ │V+1    │
└───────┘ └───────┘ └───────┘ └───────┘ └───────┘ └───────┘
```

### 5.3 Cutover Command

```bash
# Instant traffic switch via target group weights
aws elbv2 modify-listener --listener-arn $LISTENER_ARN \
  --default-actions '[
    {"Type":"forward","ForwardConfig":{
      "TargetGroups":[
        {"TargetGroupArn":"'$BLUE_TG'","Weight":0},
        {"TargetGroupArn":"'$GREEN_TG'","Weight":1}
      ]
    }}
  ]'
```

### 5.4 Rollback (< 30 seconds)

```bash
# Swap weights back
aws elbv2 modify-listener --listener-arn $LISTENER_ARN \
  --default-actions '[
    {"Type":"forward","ForwardConfig":{
      "TargetGroups":[
        {"TargetGroupArn":"'$BLUE_TG'","Weight":1},
        {"TargetGroupArn":"'$GREEN_TG'","Weight":0}
      ]
    }}
  ]'
```

---

## 6. Observability

### 6.1 Key Metrics

| Metric | Alert Threshold | Action |
|--------|-----------------|--------|
| `request_latency_p99` | > 5ms | Page on-call |
| `request_latency_p95` | > 3ms | Warning |
| `error_rate` | > 0.01% | Page on-call |
| `page_faults_major` | > 0 | Investigate (swapping) |
| `cpu_usage` | > 70% | Scale out |
| `memory_usage` | > 85% | Scale out |

### 6.2 Load Testing

```bash
# Using ghz (gRPC benchmarking tool)
ghz --insecure \
  --proto ./proto/kv.proto \
  --call kv.KVService/Get \
  --data '{"key": "{{.Key}}"}' \
  --data-file /test/keys.json \
  --concurrency 100 \
  --qps 10000 \
  --duration 300s \
  kv-nlb.internal:9090

# Expected: p99 < 5ms, 0 errors
```

---

## 7. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Snapshot build failure | Automated validation, Blue serves until fixed |
| Memory pressure during B/G | Use r6i.4xlarge (128 GB), staggered warmup |
| Unknown key returns wrong value | Always verify key match in lookup |
| NLB misconfiguration | Automated runbook, dry runs |
| Node failure | N+1 redundancy (3 nodes) |

---

## 8. Summary

- **O(1) lookups** via minimal perfect hashing
- **Predictable p99 < 5ms** via memory-mapped RAM-resident data
- **Zero-downtime updates** via AWS NLB target group switching
- **High availability** via full replication across 3 nodes
- **Simple operations** via full replication (no sharding)
