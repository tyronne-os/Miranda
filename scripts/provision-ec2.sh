#!/usr/bin/env bash
# Launch the Miranda-Engine t4g.small EC2 instance if it doesn't exist yet.
# Installs Rust and clones the repo automatically on first boot.
#
# Prerequisites: aws-setup.sh must have been run first (credentials configured).
# Usage: bash scripts/provision-ec2.sh

set -e

REGION=$(aws configure get default.region || echo "us-east-1")
KEY_NAME="beryl-miranda-key"
INSTANCE_TYPE="t4g.small"
AMI_SEARCH="al2023-ami-*-arm64"   # Amazon Linux 2023, ARM64

echo ""
echo "=== Miranda-Engine: EC2 provisioning ==="
echo "Region: $REGION | Type: $INSTANCE_TYPE"
echo ""

# --- Import or check key pair ---
PEM_PATH="$HOME/.ssh/beryl-aws-key.pem"
if [ ! -f "$PEM_PATH" ]; then
  echo "✗ PEM key not found at $PEM_PATH. Run aws-setup.sh first."
  exit 1
fi

KEY_EXISTS=$(aws ec2 describe-key-pairs --key-names "$KEY_NAME" --query "KeyPairs[0].KeyName" --output text 2>/dev/null || echo "")
if [ -z "$KEY_EXISTS" ] || [ "$KEY_EXISTS" = "None" ]; then
  echo "Importing public key as $KEY_NAME in AWS..."
  # Extract public key from the PEM, import it
  PUBLIC_KEY=$(ssh-keygen -y -f "$PEM_PATH" 2>/dev/null)
  aws ec2 import-key-pair \
    --key-name "$KEY_NAME" \
    --public-key-material "$(echo "$PUBLIC_KEY" | base64)" \
    --output text > /dev/null
  echo "✓ Key pair imported"
else
  echo "✓ Key pair $KEY_NAME already exists in AWS"
fi

# --- Security group ---
SG_NAME="miranda-engine-sg"
SG_ID=$(aws ec2 describe-security-groups \
  --filters "Name=group-name,Values=$SG_NAME" \
  --query "SecurityGroups[0].GroupId" --output text 2>/dev/null || echo "")

if [ -z "$SG_ID" ] || [ "$SG_ID" = "None" ]; then
  echo "Creating security group $SG_NAME..."
  SG_ID=$(aws ec2 create-security-group \
    --group-name "$SG_NAME" \
    --description "Miranda-Engine dev/test instance" \
    --query "GroupId" --output text)
  # SSH only from current IP
  MY_IP=$(curl -s https://checkip.amazonaws.com)
  aws ec2 authorize-security-group-ingress \
    --group-id "$SG_ID" \
    --protocol tcp --port 22 --cidr "${MY_IP}/32" > /dev/null
  echo "✓ Security group created: $SG_ID (SSH from $MY_IP only)"
else
  echo "✓ Security group already exists: $SG_ID"
fi

# --- Find AMI ---
AMI_ID=$(aws ec2 describe-images \
  --owners amazon \
  --filters "Name=name,Values=${AMI_SEARCH}" \
            "Name=state,Values=available" \
  --query "sort_by(Images, &CreationDate)[-1].ImageId" \
  --output text)
echo "✓ Latest ARM64 AMI: $AMI_ID"

# --- Launch instance ---
# User data: install Rust and clone the repo on first boot
USER_DATA=$(cat <<'USERDATA'
#!/bin/bash
set -e
yum update -y
yum install -y git gcc
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
git clone https://github.com/tyronne-os/Miranda.git /home/ec2-user/miranda-engine
chown -R ec2-user:ec2-user /home/ec2-user/miranda-engine
USERDATA
)

echo "Launching $INSTANCE_TYPE instance..."
INSTANCE_ID=$(aws ec2 run-instances \
  --image-id "$AMI_ID" \
  --instance-type "$INSTANCE_TYPE" \
  --key-name "$KEY_NAME" \
  --security-group-ids "$SG_ID" \
  --user-data "$USER_DATA" \
  --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=miranda-engine-arm64}]" \
  --query "Instances[0].InstanceId" \
  --output text)

echo "✓ Instance launched: $INSTANCE_ID"
echo "  Waiting for it to reach 'running' state (takes ~30s)..."
aws ec2 wait instance-running --instance-ids "$INSTANCE_ID"

PUBLIC_IP=$(aws ec2 describe-instances \
  --instance-ids "$INSTANCE_ID" \
  --query "Reservations[0].Instances[0].PublicIpAddress" \
  --output text)

echo "✓ Instance is running at $PUBLIC_IP"
ssh-keyscan -H "$PUBLIC_IP" >> ~/.ssh/known_hosts 2>/dev/null
echo "$PUBLIC_IP" > ~/.miranda-ec2-ip
echo "✓ IP saved to ~/.miranda-ec2-ip"

echo ""
echo "=== Provisioning complete ==="
echo "Wait ~3 minutes for the user-data bootstrap (Rust install + git clone) to finish."
echo "Then run: bash scripts/arm64-verify.sh"
