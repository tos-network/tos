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
SSL_OPTIONS=/etc/letsencrypt/options-ssl-nginx.conf
SSL_DH=/etc/letsencrypt/ssl-dhparams.pem

ensure_ssl_templates() {
  echo "==== Ensuring SSL helper templates exist ===="
  mkdir -p /etc/letsencrypt
  if [ ! -f "${SSL_OPTIONS}" ]; then
    echo "Downloading ${SSL_OPTIONS}"
    curl -fsSL "https://raw.githubusercontent.com/certbot/certbot/master/certbot_nginx/certbot_nginx/_internal/options-ssl-nginx.conf" -o "${SSL_OPTIONS}"
  fi
  if [ ! -f "${SSL_DH}" ]; then
    echo "Generating ${SSL_DH} (could take a minute)"
    openssl dhparam -out "${SSL_DH}" 2048
  fi
}

install_dependencies() {
  echo "==== 1. Installing dependencies (nginx + certbot) ===="
  apt update
  apt install -y nginx certbot python3-certbot-nginx curl openssl
}

prepare_webroot() {
  echo "==== 2. Preparing ACME challenge directory ===="
  mkdir -p "${WEBROOT}"
  chown -R www-data:www-data "${WEBROOT}"
}

create_websocket_map() {
  echo "==== 3. Creating WebSocket upgrade map ===="
  mkdir -p /etc/nginx/conf.d
  cat <<'MAPEOF' > "${WEBSOCKET_MAP}"
map $http_upgrade $connection_upgrade {
    default upgrade;
    '' close;
}
MAPEOF
}

write_nginx_site() {
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
    include ${SSL_OPTIONS};
    ssl_dhparam ${SSL_DH};

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
}

enable_site() {
  echo "==== 5. Enabling site and disabling default ===="
  rm -f /etc/nginx/sites-enabled/default
  ln -sf "${NGINX_SITE}" "${NGINX_SITE_LINK}"
}

test_and_reload_nginx() {
  echo "==== 6. Testing Nginx configuration and reloading ===="
  nginx -t
  systemctl reload nginx
}

obtain_certificate() {
  echo "==== 7. Requesting Let's Encrypt certificate ===="
  if [ ! -f "/etc/letsencrypt/live/${DOMAIN}/fullchain.pem" ]; then
    certbot certonly \
      --webroot -w "${WEBROOT}" \
      --non-interactive --agree-tos \
      --email "${EMAIL}" \
      -d "${DOMAIN}"
  else
    echo "Certificate already exists, skipping certbot request."
  fi
}

reload_with_cert() {
  echo "==== 8. Reloading Nginx to activate certificate ===="
  nginx -t
  systemctl reload nginx
}

configure_firewall() {
  echo "==== 9. Configuring firewall (UFW) if present ===="
  if command -v ufw >/dev/null 2>&1; then
    ufw allow 'Nginx Full' || true
  fi
}

main() {
  install_dependencies
  prepare_webroot
  create_websocket_map
  ensure_ssl_templates
  write_nginx_site
  enable_site
  test_and_reload_nginx || true
  obtain_certificate
  reload_with_cert
  configure_firewall

  echo "==== Done ===="
  echo "Verify with:"
  echo "  curl -I https://${DOMAIN}/json_rpc"
  echo "  wscat -c wss://${DOMAIN}/json_rpc"
  echo
  echo "Certificates renew automatically via certbot timers; run 'certbot renew --dry-run' to check."
}

main "$@"
