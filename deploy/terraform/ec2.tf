# IAM role for EC2 instances
resource "aws_iam_role" "kv_server" {
  name = "${var.project_name}-server-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      }
    ]
  })

  tags = local.common_tags
}

# IAM policy for S3 access
resource "aws_iam_role_policy" "kv_server_s3" {
  name = "${var.project_name}-s3-policy"
  role = aws_iam_role.kv_server.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "s3:GetObject",
          "s3:ListBucket"
        ]
        Resource = [
          aws_s3_bucket.snapshots.arn,
          "${aws_s3_bucket.snapshots.arn}/*"
        ]
      }
    ]
  })
}

# IAM instance profile
resource "aws_iam_instance_profile" "kv_server" {
  name = "${var.project_name}-server-profile"
  role = aws_iam_role.kv_server.name

  tags = local.common_tags
}

# Launch template for KV servers
resource "aws_launch_template" "kv_server" {
  name_prefix   = "${var.project_name}-server-"
  image_id      = data.aws_ami.amazon_linux_2023.id
  instance_type = var.instance_type

  iam_instance_profile {
    name = aws_iam_instance_profile.kv_server.name
  }

  network_interfaces {
    associate_public_ip_address = false
    security_groups             = [aws_security_group.kv_server.id]
    subnet_id                   = module.vpc.private_subnets[0]
  }

  placement {
    group_name = aws_placement_group.kv_cluster.name
  }

  block_device_mappings {
    device_name = "/dev/xvda"

    ebs {
      volume_size           = var.root_volume_size
      volume_type           = "gp3"
      iops                  = var.root_volume_iops
      encrypted             = true
      delete_on_termination = true
    }
  }

  # Disable swap for predictable latency
  user_data = base64encode(<<-EOF
    #!/bin/bash
    set -e

    # Disable swap
    swapoff -a
    sed -i '/swap/d' /etc/fstab

    # Set vm parameters for low latency
    echo 'vm.swappiness=0' >> /etc/sysctl.conf
    echo 'vm.overcommit_memory=1' >> /etc/sysctl.conf
    sysctl -p

    # Install dependencies
    dnf install -y aws-cli

    # Create data directory
    mkdir -p /data/snapshots

    # Enable and start the KV server (systemd unit should be deployed separately)
    # systemctl enable kv-server
    # systemctl start kv-server
  EOF
  )

  monitoring {
    enabled = true
  }

  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
  }

  tags = local.common_tags

  lifecycle {
    create_before_destroy = true
  }
}

# Blue pool instances
resource "aws_instance" "kv_blue" {
  count = var.enable_blue_pool ? var.node_count : 0

  launch_template {
    id      = aws_launch_template.kv_server.id
    version = "$Latest"
  }

  tags = merge(local.common_tags, {
    Name = "${var.project_name}-blue-${count.index}"
    Pool = "blue"
  })
}

# Green pool instances
resource "aws_instance" "kv_green" {
  count = var.enable_green_pool ? var.node_count : 0

  launch_template {
    id      = aws_launch_template.kv_server.id
    version = "$Latest"
  }

  tags = merge(local.common_tags, {
    Name = "${var.project_name}-green-${count.index}"
    Pool = "green"
  })
}
