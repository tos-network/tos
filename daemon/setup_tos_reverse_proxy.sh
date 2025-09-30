#!/usr/bin/env bash
# setup_tos_reverse_proxy.sh
# Usage:
#   sudo bash setup_tos_reverse_proxy.sh <domain> <email> [backend_host] [backend_port]
# Example:
#   sudo bash setup_tos_reverse_proxy.sh testnet.tos.network admin@tos.network 127.0.0.1 8080

set -euo pipefail

DOMAIN=${1:-testnet.tos.network}
EMAIL=${2:-admin@tos.network}
BACKEND_HOST=${3:-127.0.0.1}
BACKEND_PORT=${4:-8080}

WEBROOT=/var/www/letsencrypt
NGINX_SITE=/etc/nginx/sites-available/${DOMAIN}.conf
NGINX_SITE_LINK=/etc/nginx/sites-enabled/${DOMAIN}.conf
WEBSOCKET_MAP=/etc/nginx/conf.d/websocket_upgrade.conf

echo "==== 1. Installing dependencies (nginx + certbot) ===="
apt update
apt install -y nginx certbot python3-certbot-nginx

echo "==== 2. Preparing ACME challenge directory ===="
mkdir -p "${WEBROOT}"
chown -R www-data:www-data "${WEBROOT}"

echo "==== 3. Creating WebSocket upgrade map ===="
cat <<'MAPEOF' > "${WEBSOCKET_MAP}"
map $http_upgrade $connection_upgrade {
    default upgrade;
    '' close;
}
MAPEOF

echo "==== 4. Generating Nginx site configuration (${NGINX_SITE}) ===="
cat <<NGINXCONF > "${NGINX_SITE}"
server {
    listen 80;
    listen [::]:80;
    server_name ${DOMAIN};

    location ^~ /.well-known/acme-challenge/ {
        root ${WEBROOT};
    }

    location / {
        return 301 https://\$host\$request_uri;
    }
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name ${DOMAIN};

    ssl_certificate /etc/letsencrypt/live/${DOMAIN}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/${DOMAIN}/privkey.pem;
    include /etc/letsencrypt/options-ssl-nginx.conf;
    ssl_dhparam /etc/letsencrypt/ssl-dhparams.pem;

    location / {
        proxy_pass http://${BACKEND_HOST}:${BACKEND_PORT};
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;

        proxy_http_version 1.1;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection \$connection_upgrade;
    }
}
NGINXCONF

echo "==== 5. Enabling site and disabling default ===="
rm -f /etc/nginx/sites-enabled/default
ln -sf "${NGINX_SITE}" "${NGINX_SITE_LINK}"

echo "==== 6. Testing Nginx configuration and reloading ===="
nginx -t
systemctl reload nginx

echo "==== 7. Requesting Let's Encrypt certificate ===="
if [ ! -f "/etc/letsencrypt/live/${DOMAIN}/fullchain.pem" ]; then
    certbot certonly \
        --webroot -w "${WEBROOT}" \
        --non-interactive --agree-tos \
        --email "${EMAIL}" \
        -d "${DOMAIN}"
fi

echo "==== 8. Reloading Nginx to activate certificate ===="
nginx -t
systemctl reload nginx

echo "==== 9. Configuring firewall (UFW) if present ===="
if command -v ufw >/dev/null 2>&1; then
    ufw allow 'Nginx Full' || true
fi

echo "==== Done ===="
echo "Verify with:"
echo "  curl -I https://${DOMAIN}/json_rpc"
echo "  wscat -c wss://${DOMAIN}/json_rpc"
echo
echo "Certificates renew automatically via certbot timers; run 'certbot renew --dry-run' to check."
