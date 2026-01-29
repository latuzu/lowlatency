#!/bin/bash
# Deploy snapshot to a pool of nodes
# Usage: ./deploy-snapshot.sh [blue|green] <snapshot-version>

set -euo pipefail

POOL=${1:-}
VERSION=${2:-}

if [[ -z "$POOL" ]] || [[ -z "$VERSION" ]]; then
    echo "Usage: $0 [blue|green] <snapshot-version>"
    echo "Example: $0 green v20240115"
    exit 1
fi

# Get configuration from Terraform
BUCKET=$(terraform -chdir=../terraform output -raw snapshot_bucket_name)

if [[ "$POOL" == "blue" ]]; then
    INSTANCE_IPS=$(terraform -chdir=../terraform output -json blue_instance_private_ips | jq -r '.[]')
elif [[ "$POOL" == "green" ]]; then
    INSTANCE_IPS=$(terraform -chdir=../terraform output -json green_instance_private_ips | jq -r '.[]')
else
    echo "Invalid pool: $POOL"
    exit 1
fi

echo "Deploying snapshot $VERSION to $POOL pool"
echo "S3 Bucket: $BUCKET"
echo "Target instances: $INSTANCE_IPS"

# Deploy to each instance
for IP in $INSTANCE_IPS; do
    echo ""
    echo "=== Deploying to $IP ==="

    # Download snapshot from S3
    echo "Downloading snapshot..."
    ssh -o StrictHostKeyChecking=no ec2-user@$IP "
        aws s3 cp s3://$BUCKET/$VERSION/snapshot.bin /data/snapshots/$VERSION/snapshot.bin
    "

    # Verify checksum
    echo "Verifying checksum..."
    ssh -o StrictHostKeyChecking=no ec2-user@$IP "
        aws s3 cp s3://$BUCKET/$VERSION/checksum.sha256 /tmp/checksum.sha256
        cd /data/snapshots/$VERSION && sha256sum -c /tmp/checksum.sha256
    "

    # Warmup (preload into page cache)
    echo "Warming up snapshot..."
    ssh -o StrictHostKeyChecking=no ec2-user@$IP "
        vmtouch -t /data/snapshots/$VERSION/snapshot.bin || cat /data/snapshots/$VERSION/snapshot.bin > /dev/null
    "

    # Restart service with new snapshot
    echo "Restarting kv-server..."
    ssh -o StrictHostKeyChecking=no ec2-user@$IP "
        sudo systemctl stop kv-server || true
        sudo systemctl start kv-server
    "

    # Wait for health check
    echo "Waiting for health check..."
    for i in {1..30}; do
        if ssh -o StrictHostKeyChecking=no ec2-user@$IP "curl -sf http://localhost:9091/health > /dev/null"; then
            echo "Node $IP is healthy!"
            break
        fi
        echo "  Waiting... ($i/30)"
        sleep 2
    done

    echo "=== Completed $IP ==="
done

echo ""
echo "Deployment complete!"
echo "Run validation before cutover:"
echo "  ./validate-pool.sh $POOL"
