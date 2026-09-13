#!/usr/bin/env bash
# Run on your Mac, from the project folder.
set -euo pipefail

HOST="${HOST:-root@109.205.178.227}"
APP_DIR="${APP_DIR:-/opt/datacooking}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "upload ${ROOT} -> ${HOST}:${APP_DIR}"
ssh "${HOST}" "mkdir -p ${APP_DIR}"
rsync -az --delete \
  --exclude target \
  --exclude .git \
  --exclude .idea \
  --exclude .DS_Store \
  "${ROOT}/" "${HOST}:${APP_DIR}/"
echo "uploaded."
echo "next on the server:"
echo "  ssh ${HOST}"
echo "  cd ${APP_DIR} && DOMAIN=datacooking.dev bash deploy/contabo.sh"
