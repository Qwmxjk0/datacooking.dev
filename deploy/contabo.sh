#!/usr/bin/env bash
# Run on the Contabo VPS as root.
set -euo pipefail

DOMAIN="${DOMAIN:-datacooking.dev}"
APP_DIR="${APP_DIR:-/opt/datacooking}"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "run as root: sudo DOMAIN=${DOMAIN} bash deploy/contabo.sh"
  exit 1
fi

if [[ ! -f "${APP_DIR}/docker-compose.prod.yml" ]]; then
  echo "missing ${APP_DIR}/docker-compose.prod.yml"
  echo "upload the project first from your Mac:"
  echo "  HOST=root@YOUR_VPS_IP bash deploy/upload.sh"
  exit 1
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update -y
apt-get install -y ca-certificates curl ufw

if ! command -v docker >/dev/null 2>&1; then
  curl -fsSL https://get.docker.com | sh
fi

ufw allow OpenSSH
ufw allow 80/tcp
ufw allow 443/tcp
ufw --force enable

cd "${APP_DIR}"
DOMAIN="${DOMAIN}" docker compose -f docker-compose.prod.yml up -d --build

echo
echo "waiting for HTTP..."
for i in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1/health" >/dev/null 2>&1; then
    echo "local health ok"
    break
  fi
  sleep 2
done

echo
echo "DNS for ${DOMAIN} must be an A record to this VPS, Cloudflare proxy OFF (grey cloud)."
echo "then: curl -fsS https://${DOMAIN}/health"
echo "logs: docker compose -f ${APP_DIR}/docker-compose.prod.yml logs -f"
