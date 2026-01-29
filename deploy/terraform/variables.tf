variable "project_name" {
  description = "Name of the project"
  type        = string
  default     = "lowlatency-kv"
}

variable "environment" {
  description = "Environment (dev, staging, prod)"
  type        = string
  default     = "prod"
}

variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "us-east-1"
}

variable "availability_zone" {
  description = "Single AZ for low-latency deployment"
  type        = string
  default     = "us-east-1a"
}

variable "vpc_cidr" {
  description = "VPC CIDR block"
  type        = string
  default     = "10.0.0.0/16"
}

variable "instance_type" {
  description = "EC2 instance type for KV servers"
  type        = string
  default     = "r6i.4xlarge" # 16 vCPU, 128 GB RAM
}

variable "node_count" {
  description = "Number of nodes per pool (blue/green)"
  type        = number
  default     = 3
}

variable "root_volume_size" {
  description = "Root EBS volume size in GB"
  type        = number
  default     = 100
}

variable "root_volume_iops" {
  description = "Root EBS volume IOPS"
  type        = number
  default     = 3000
}

variable "snapshot_bucket_name" {
  description = "S3 bucket name for snapshots (leave empty for auto-generated)"
  type        = string
  default     = ""
}

variable "enable_blue_pool" {
  description = "Enable blue pool instances"
  type        = bool
  default     = true
}

variable "enable_green_pool" {
  description = "Enable green pool instances"
  type        = bool
  default     = true
}

variable "blue_weight" {
  description = "Traffic weight for blue pool (0-100)"
  type        = number
  default     = 100
}

variable "green_weight" {
  description = "Traffic weight for green pool (0-100)"
  type        = number
  default     = 0
}
