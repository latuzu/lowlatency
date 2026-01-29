# Production Deployment Guide

This guide covers deploying the low-latency key-value store in a production environment.

## System Requirements

### Hardware
- **CPU:** Modern multi-core processor (4+ cores recommended)
- **RAM:** Minimum 8GB, recommended 16GB+ for 100M records
  - Approximately 100 bytes per key for the hash index
  - 100M records = ~10GB for index + OS overhead
- **Storage:** 
  - SSD or NVMe storage strongly recommended
  - 32GB minimum for 100M records (320 bytes × 100M)
- **Network:** Low-latency network (same datacenter/AZ)

### Software
- **OS:** Linux (Ubuntu 20.04+ or similar)
- **Go:** 1.21 or higher
- **Docker:** (optional) 20.10+ for containerized deployment

## Deployment Options

### 1. Native Deployment

#### Step 1: Prepare Data File

Generate the data file with 100 million records:

```bash
./generate -count 100000000 -output /data/production.bin
```

This will create a ~32GB file. Ensure sufficient disk space.

#### Step 2: System Tuning

```bash
# Increase file descriptor limit
ulimit -n 65535

# Increase maximum number of memory map areas
sudo sysctl -w vm.max_map_count=262144

# For better memory-mapped file performance
sudo sysctl -w vm.swappiness=10

# TCP tuning for low latency
sudo sysctl -w net.ipv4.tcp_low_latency=1
sudo sysctl -w net.core.somaxconn=4096
```

Make these changes persistent by adding them to `/etc/sysctl.conf`.

#### Step 3: Start Server

```bash
./server -data /data/production.bin -port 8080
```

For production, use a process manager like systemd:

```ini
# /etc/systemd/system/lowlatency-kv.service
[Unit]
Description=Low Latency Key-Value Store
After=network.target

[Service]
Type=simple
User=kvstore
WorkingDirectory=/opt/lowlatency
ExecStart=/opt/lowlatency/server -data /data/production.bin -port 8080
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl enable lowlatency-kv
sudo systemctl start lowlatency-kv
```

### 2. Docker Deployment

#### Build Image

```bash
docker build -t lowlatency-kv:latest .
```

#### Run Container

```bash
# First, generate data
docker run --rm -v $(pwd)/data:/data lowlatency-kv:latest \
  ./generate -count 100000000 -output /data/production.bin

# Run server
docker run -d \
  --name lowlatency-kv \
  -p 8080:8080 \
  -v $(pwd)/data:/data \
  --ulimit nofile=65535:65535 \
  --memory=16g \
  lowlatency-kv:latest \
  ./server -data /data/production.bin -port 8080
```

#### Using Docker Compose

```bash
# Generate data
docker-compose run server ./generate -count 100000000 -output /data/production.bin

# Start service
docker-compose up -d
```

### 3. Kubernetes Deployment

Create deployment manifest:

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: kv-data-pvc
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 40Gi
  storageClassName: ssd-storage

---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: lowlatency-kv
spec:
  replicas: 3
  selector:
    matchLabels:
      app: lowlatency-kv
  template:
    metadata:
      labels:
        app: lowlatency-kv
    spec:
      containers:
      - name: kv-server
        image: lowlatency-kv:latest
        ports:
        - containerPort: 8080
        resources:
          requests:
            memory: "12Gi"
            cpu: "2"
          limits:
            memory: "16Gi"
            cpu: "4"
        volumeMounts:
        - name: data
          mountPath: /data
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: kv-data-pvc

---
apiVersion: v1
kind: Service
metadata:
  name: lowlatency-kv-service
spec:
  selector:
    app: lowlatency-kv
  ports:
    - protocol: TCP
      port: 8080
      targetPort: 8080
  type: LoadBalancer
```

Deploy:
```bash
kubectl apply -f k8s-deployment.yaml
```

## Load Balancing

For horizontal scaling, deploy multiple instances behind a load balancer.

### NGINX Configuration

```nginx
upstream kv_backend {
    least_conn;
    server kv-server1:8080 max_fails=3 fail_timeout=30s;
    server kv-server2:8080 max_fails=3 fail_timeout=30s;
    server kv-server3:8080 max_fails=3 fail_timeout=30s;
    keepalive 256;
}

server {
    listen 80;
    server_name kv.example.com;

    location / {
        proxy_pass http://kv_backend;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_connect_timeout 1s;
        proxy_send_timeout 1s;
        proxy_read_timeout 1s;
    }

    location /health {
        proxy_pass http://kv_backend;
        access_log off;
    }
}
```

## Monitoring

### Health Checks

The `/health` endpoint provides basic health information:

```bash
curl http://localhost:8080/health
# Output: OK - 100000000 records loaded
```

### Metrics

Add Prometheus metrics by integrating the prometheus client library.

Example metrics to track:
- Request rate (QPS)
- Response time percentiles (p50, p95, p99)
- Error rate
- Memory usage
- CPU usage

### Logging

Configure structured logging for production:

```go
import "log/slog"

logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
logger.Info("Request processed", "key", key, "duration_ms", duration)
```

## Performance Tuning

### OS-Level Tuning

```bash
# Transparent Huge Pages (THP) - test if it helps your workload
echo always > /sys/kernel/mm/transparent_hugepage/enabled

# CPU governor for performance
sudo cpupower frequency-set -g performance

# IRQ affinity for network card
# Pin interrupts to specific CPUs to reduce latency variance
```

### Application-Level Tuning

1. **Connection Pooling:** Configure HTTP client connection pools properly
2. **Keep-Alive:** Use HTTP keep-alive to reduce connection overhead
3. **Kernel Bypass:** Consider using DPDK or similar for ultra-low latency

## Disaster Recovery

### Backup Strategy

```bash
# The data file is read-only, so simple file copy works
cp /data/production.bin /backup/production-$(date +%Y%m%d).bin
```

### Replication

Replicate the data file to multiple servers:

```bash
# Using rsync
rsync -av --progress /data/production.bin backup-server:/data/

# Or use distributed file systems like Ceph, GlusterFS
```

## Security Considerations

1. **Network Security:**
   - Deploy in private network/VPC
   - Use security groups/firewall rules
   - Consider mTLS for inter-service communication

2. **Access Control:**
   - Add authentication layer (API keys, OAuth)
   - Use API gateway for rate limiting

3. **DDoS Protection:**
   - Deploy behind CDN/DDoS protection service
   - Implement rate limiting

Example rate limiting with NGINX:

```nginx
limit_req_zone $binary_remote_addr zone=kv_limit:10m rate=10000r/s;

server {
    location / {
        limit_req zone=kv_limit burst=1000 nodelay;
        proxy_pass http://kv_backend;
    }
}
```

## Troubleshooting

### High Latency

1. Check system load and CPU usage
2. Verify storage I/O performance
3. Check network latency
4. Review memory pressure (swapping)
5. Check if data is in OS page cache

### Out of Memory

1. Reduce record count or shard data
2. Increase system RAM
3. Optimize index data structure

### Connection Refused

1. Check if server is running: `systemctl status lowlatency-kv`
2. Verify port is open: `netstat -tulpn | grep 8080`
3. Check firewall rules

## Performance Verification

Run a production-like load test:

```bash
# Generate 10M records for testing
./generate -count 10000000 -output test.bin

# Start server
./server -data test.bin

# Run benchmark
./benchmark -url http://localhost:8080 -qps 10000 -duration 60 -keys 10000000
```

Expected results:
- p50 latency: < 1ms
- p95 latency: < 2ms
- p99 latency: < 5ms
- p99.9 latency: < 10ms

## Cost Optimization

### AWS Deployment Example

For 100M records deployment:

- **Instance Type:** r6i.xlarge (32 GiB RAM, 4 vCPU)
- **Storage:** 40 GB gp3 SSD
- **Network:** Enhanced networking enabled
- **Estimated Cost:** ~$240/month per instance

### Cost Reduction Strategies

1. Use spot instances for non-critical environments
2. Share data file across read replicas using EFS
3. Use reserved instances for production
4. Right-size instance based on actual load

## Scaling Guidelines

| QPS Target | Instances | Load Balancer |
|------------|-----------|---------------|
| 10K        | 1-2       | Optional      |
| 50K        | 3-5       | Required      |
| 100K       | 6-10      | Required      |
| 500K       | 30-50     | Required      |

Each instance can handle approximately 10-20K QPS depending on hardware and network conditions.
