# Network Load Balancer
resource "aws_lb" "kv" {
  name               = "${var.project_name}-nlb"
  internal           = true
  load_balancer_type = "network"
  subnets            = module.vpc.private_subnets

  enable_cross_zone_load_balancing = false # Same AZ only for low latency

  tags = merge(local.common_tags, {
    Name = "${var.project_name}-nlb"
  })
}

# Blue target group
resource "aws_lb_target_group" "blue" {
  name        = "${var.project_name}-blue"
  port        = 9090
  protocol    = "TCP"
  vpc_id      = module.vpc.vpc_id
  target_type = "instance"

  health_check {
    enabled             = true
    protocol            = "TCP"
    port                = "9091"
    healthy_threshold   = 2
    unhealthy_threshold = 2
    interval            = 10
  }

  tags = merge(local.common_tags, {
    Pool = "blue"
  })
}

# Green target group
resource "aws_lb_target_group" "green" {
  name        = "${var.project_name}-green"
  port        = 9090
  protocol    = "TCP"
  vpc_id      = module.vpc.vpc_id
  target_type = "instance"

  health_check {
    enabled             = true
    protocol            = "TCP"
    port                = "9091"
    healthy_threshold   = 2
    unhealthy_threshold = 2
    interval            = 10
  }

  tags = merge(local.common_tags, {
    Pool = "green"
  })
}

# NLB listener with weighted routing
resource "aws_lb_listener" "kv" {
  load_balancer_arn = aws_lb.kv.arn
  port              = 9090
  protocol          = "TCP"

  default_action {
    type = "forward"

    forward {
      target_group {
        arn    = aws_lb_target_group.blue.arn
        weight = var.blue_weight
      }

      target_group {
        arn    = aws_lb_target_group.green.arn
        weight = var.green_weight
      }
    }
  }
}

# Attach blue instances to blue target group
resource "aws_lb_target_group_attachment" "blue" {
  count            = var.enable_blue_pool ? var.node_count : 0
  target_group_arn = aws_lb_target_group.blue.arn
  target_id        = aws_instance.kv_blue[count.index].id
  port             = 9090
}

# Attach green instances to green target group
resource "aws_lb_target_group_attachment" "green" {
  count            = var.enable_green_pool ? var.node_count : 0
  target_group_arn = aws_lb_target_group.green.arn
  target_id        = aws_instance.kv_green[count.index].id
  port             = 9090
}
