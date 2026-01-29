#!/bin/bash
# Blue/Green cutover script
# Usage: ./cutover.sh [blue|green]

set -euo pipefail

TARGET=${1:-}

if [[ -z "$TARGET" ]]; then
    echo "Usage: $0 [blue|green]"
    echo "  blue  - Switch traffic to blue pool"
    echo "  green - Switch traffic to green pool"
    exit 1
fi

# Get ARNs from Terraform output
LISTENER_ARN=$(terraform -chdir=../terraform output -raw listener_arn)
BLUE_TG_ARN=$(terraform -chdir=../terraform output -raw blue_target_group_arn)
GREEN_TG_ARN=$(terraform -chdir=../terraform output -raw green_target_group_arn)

echo "Listener ARN: $LISTENER_ARN"
echo "Blue Target Group: $BLUE_TG_ARN"
echo "Green Target Group: $GREEN_TG_ARN"

if [[ "$TARGET" == "blue" ]]; then
    echo "Switching traffic to BLUE pool..."
    BLUE_WEIGHT=100
    GREEN_WEIGHT=0
elif [[ "$TARGET" == "green" ]]; then
    echo "Switching traffic to GREEN pool..."
    BLUE_WEIGHT=0
    GREEN_WEIGHT=100
else
    echo "Invalid target: $TARGET"
    exit 1
fi

# Perform the switch
aws elbv2 modify-listener --listener-arn "$LISTENER_ARN" \
    --default-actions "[
        {\"Type\":\"forward\",\"ForwardConfig\":{
            \"TargetGroups\":[
                {\"TargetGroupArn\":\"$BLUE_TG_ARN\",\"Weight\":$BLUE_WEIGHT},
                {\"TargetGroupArn\":\"$GREEN_TG_ARN\",\"Weight\":$GREEN_WEIGHT}
            ]
        }}
    ]"

echo "Traffic switched to $TARGET pool successfully!"

# Verify the switch
echo "Verifying listener configuration..."
aws elbv2 describe-listeners --listener-arns "$LISTENER_ARN" \
    --query 'Listeners[0].DefaultActions[0].ForwardConfig.TargetGroups[*].[TargetGroupArn,Weight]' \
    --output table
