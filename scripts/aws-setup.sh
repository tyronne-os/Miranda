#!/usr/bin/env bash
# One-time AWS credential + SSH key setup for Miranda-Engine.
# Run this script ONCE from your own terminal. Your keys never appear
# in any chat session or log — they go from your clipboard → this script → disk only.
#
# Usage: bash scripts/aws-setup.sh

set -e

echo ""
echo "=== Miranda-Engine: AWS credential setup ==="
echo "Your keys will be written to ~/.aws/credentials and ~/.aws/config."
echo "They will not be printed to the screen."
echo ""

# --- AWS credentials ---
printf "AWS Access Key ID: "
read -r -s AWS_ACCESS_KEY_ID
echo ""

printf "AWS Secret Access Key: "
read -r -s AWS_SECRET_ACCESS_KEY
echo ""

printf "AWS default region [us-east-1]: "
read -r AWS_REGION
AWS_REGION="${AWS_REGION:-us-east-1}"

aws configure set aws_access_key_id     "$AWS_ACCESS_KEY_ID"
aws configure set aws_secret_access_key "$AWS_SECRET_ACCESS_KEY"
aws configure set default.region        "$AWS_REGION"
aws configure set default.output        "json"

echo ""
echo "Verifying credentials with AWS STS..."
IDENTITY=$(aws sts get-caller-identity 2>&1)
if echo "$IDENTITY" | grep -q '"UserId"'; then
  ACCOUNT=$(echo "$IDENTITY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['Account'])")
  ARN=$(echo "$IDENTITY"     | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['Arn'])")
  echo "✓ Connected. Account: $ACCOUNT | ARN: $ARN"
else
  echo "✗ Credential verification failed. Output was:"
  echo "$IDENTITY"
  exit 1
fi

# --- SSH PEM key ---
echo ""
echo "=== SSH key setup ==="
echo "Paste your EC2 PEM key below."
echo "Start pasting now, then press Ctrl+D on a NEW blank line when done:"
echo ""

mkdir -p ~/.ssh
cat > ~/.ssh/beryl-aws-key.pem
chmod 400 ~/.ssh/beryl-aws-key.pem
echo "✓ Key saved to ~/.ssh/beryl-aws-key.pem (chmod 400)"

# --- EC2 instance discovery ---
echo ""
echo "=== EC2 instance discovery ==="
echo "Looking for a running t4g.small or t3.small in $AWS_REGION..."
EC2_IP=$(aws ec2 describe-instances \
  --filters "Name=instance-state-name,Values=running" \
            "Name=instance-type,Values=t4g.small,t3.small" \
  --query "Reservations[0].Instances[0].PublicIpAddress" \
  --output text 2>/dev/null || echo "")

if [ -n "$EC2_IP" ] && [ "$EC2_IP" != "None" ]; then
  echo "✓ Found instance at $EC2_IP"
  # Trust the host key
  ssh-keyscan -H "$EC2_IP" >> ~/.ssh/known_hosts 2>/dev/null
  echo "✓ Host key added to ~/.ssh/known_hosts"
  # Save IP for the verify script
  echo "$EC2_IP" > ~/.miranda-ec2-ip
  echo "✓ Instance IP saved to ~/.miranda-ec2-ip"
else
  echo "  No running instance found automatically."
  printf "  Enter the EC2 instance public IP manually (or leave blank to skip): "
  read -r MANUAL_IP
  if [ -n "$MANUAL_IP" ]; then
    ssh-keyscan -H "$MANUAL_IP" >> ~/.ssh/known_hosts 2>/dev/null
    echo "✓ Host key added for $MANUAL_IP"
    echo "$MANUAL_IP" > ~/.miranda-ec2-ip
    echo "✓ Instance IP saved to ~/.miranda-ec2-ip"
  else
    echo "  Skipped. Run: echo '<ip>' > ~/.miranda-ec2-ip before running arm64-verify.sh"
  fi
fi

echo ""
echo "=== Setup complete ==="
echo "Run 'bash scripts/arm64-verify.sh' to start the ARM64 verification."
