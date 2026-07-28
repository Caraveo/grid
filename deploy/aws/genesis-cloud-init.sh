#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends ca-certificates certbot curl jq nginx python3-certbot-nginx

install -d -m 0755 /opt/grid/bin
install -d -m 0700 -o ubuntu -g ubuntu /var/lib/grid
install -d -m 0755 /etc/grid

cat >/etc/sysctl.d/60-grid-network.conf <<'EOF'
net.ipv4.tcp_syncookies=1
net.ipv4.conf.all.rp_filter=1
net.ipv4.conf.default.rp_filter=1
EOF
sysctl --system

cat >/etc/motd <<'EOF'
GRID Genesis node

P2P: TCP/9900
Signed truth API: TCP/9100
EOF

touch /var/lib/cloud/instance/grid-bootstrap-complete
