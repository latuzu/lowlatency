output "vpc_id" {
  description = "VPC ID"
  value       = module.vpc.vpc_id
}

output "private_subnets" {
  description = "Private subnet IDs"
  value       = module.vpc.private_subnets
}

output "nlb_dns_name" {
  description = "NLB DNS name for client connections"
  value       = aws_lb.kv.dns_name
}

output "nlb_arn" {
  description = "NLB ARN"
  value       = aws_lb.kv.arn
}

output "listener_arn" {
  description = "NLB listener ARN (for blue/green switching)"
  value       = aws_lb_listener.kv.arn
}

output "blue_target_group_arn" {
  description = "Blue target group ARN"
  value       = aws_lb_target_group.blue.arn
}

output "green_target_group_arn" {
  description = "Green target group ARN"
  value       = aws_lb_target_group.green.arn
}

output "blue_instance_ids" {
  description = "Blue pool instance IDs"
  value       = aws_instance.kv_blue[*].id
}

output "green_instance_ids" {
  description = "Green pool instance IDs"
  value       = aws_instance.kv_green[*].id
}

output "blue_instance_private_ips" {
  description = "Blue pool instance private IPs"
  value       = aws_instance.kv_blue[*].private_ip
}

output "green_instance_private_ips" {
  description = "Green pool instance private IPs"
  value       = aws_instance.kv_green[*].private_ip
}

output "snapshot_bucket_name" {
  description = "S3 bucket name for snapshots"
  value       = aws_s3_bucket.snapshots.id
}

output "snapshot_bucket_arn" {
  description = "S3 bucket ARN for snapshots"
  value       = aws_s3_bucket.snapshots.arn
}

# Cutover command helper
output "cutover_to_green_command" {
  description = "AWS CLI command to switch traffic to green"
  value       = <<-EOT
    aws elbv2 modify-listener --listener-arn ${aws_lb_listener.kv.arn} \
      --default-actions '[
        {"Type":"forward","ForwardConfig":{
          "TargetGroups":[
            {"TargetGroupArn":"${aws_lb_target_group.blue.arn}","Weight":0},
            {"TargetGroupArn":"${aws_lb_target_group.green.arn}","Weight":100}
          ]
        }}
      ]'
  EOT
}

output "cutover_to_blue_command" {
  description = "AWS CLI command to switch traffic to blue"
  value       = <<-EOT
    aws elbv2 modify-listener --listener-arn ${aws_lb_listener.kv.arn} \
      --default-actions '[
        {"Type":"forward","ForwardConfig":{
          "TargetGroups":[
            {"TargetGroupArn":"${aws_lb_target_group.blue.arn}","Weight":100},
            {"TargetGroupArn":"${aws_lb_target_group.green.arn}","Weight":0}
          ]
        }}
      ]'
  EOT
}
