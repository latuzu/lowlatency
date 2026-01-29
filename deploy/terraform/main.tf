terraform {
  required_version = ">= 1.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

# Data sources
data "aws_caller_identity" "current" {}
data "aws_availability_zones" "available" {}

data "aws_ami" "amazon_linux_2023" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-*-x86_64"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

# VPC
module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 5.0"

  name = "${var.project_name}-vpc"
  cidr = var.vpc_cidr

  azs             = [var.availability_zone]
  private_subnets = [cidrsubnet(var.vpc_cidr, 8, 1)]
  public_subnets  = [cidrsubnet(var.vpc_cidr, 8, 100)]

  enable_nat_gateway     = true
  single_nat_gateway     = true
  enable_dns_hostnames   = true
  enable_dns_support     = true

  tags = local.common_tags
}

# Placement group for low-latency networking
resource "aws_placement_group" "kv_cluster" {
  name     = "${var.project_name}-cluster"
  strategy = "cluster"

  tags = local.common_tags
}

# Security group for KV servers
resource "aws_security_group" "kv_server" {
  name        = "${var.project_name}-server-sg"
  description = "Security group for KV server instances"
  vpc_id      = module.vpc.vpc_id

  # gRPC port (internal only)
  ingress {
    from_port   = 9090
    to_port     = 9090
    protocol    = "tcp"
    cidr_blocks = [var.vpc_cidr]
    description = "gRPC from VPC"
  }

  # Metrics port (internal only)
  ingress {
    from_port   = 9091
    to_port     = 9091
    protocol    = "tcp"
    cidr_blocks = [var.vpc_cidr]
    description = "Metrics from VPC"
  }

  # SSH (from bastion only, if needed)
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [var.vpc_cidr]
    description = "SSH from VPC"
  }

  # All outbound
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    description = "Allow all outbound"
  }

  tags = merge(local.common_tags, {
    Name = "${var.project_name}-server-sg"
  })
}

locals {
  common_tags = {
    Project     = var.project_name
    Environment = var.environment
    ManagedBy   = "terraform"
  }
}
