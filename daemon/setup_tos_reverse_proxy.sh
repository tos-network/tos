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

# Embedded fallback templates to avoid network dependencies
FALLBACK_SSL_OPTIONS='map $sent_http_content_type $expires {
    default                    off;
    text/html                  epoch;
    text/html; charset=utf-8   epoch;
    text/plain                 max;
    text/plain; charset=utf-8  max;
    text/css                   max;
    text/css; charset=utf-8    max;
    application/json           off;
    application/javascript     epoch;
    ~image/                    max;
}

ssl_session_cache   shared:SSL:10m;
ssl_session_timeout 10m;
ssl_session_tickets off;

ssl_protocols TLSv1.2 TLSv1.3;
ssl_prefer_server_ciphers off;
ssl_ciphers 'ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:DHE-RSA-AES128-GCM-SHA256:DHE-RSA-AES256-GCM-SHA384';
ssl_ecdh_curve secp384r1;
ssl_stapling on;
ssl_stapling_verify on;
resolver 1.1.1.1 1.0.0.1 valid=300s;
resolver_timeout 5s;
add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;
add_header X-Content-Type-Options nosniff;
add_header X-Frame-Options DENY;
add_header Referrer-Policy no-referrer;
add_header X-XSS-Protection "1; mode=block";
'

install_dependencies() {
  echo "==== 1. Installing dependencies (nginx + certbot) ===="
  apt update
  apt install -y nginx certbot python3-certbot-nginx curl openssl python3
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

ensure_ssl_templates() {
  echo "==== Ensuring SSL helper templates exist ===="
  mkdir -p /etc/letsencrypt

  if [ ! -f "${SSL_OPTIONS}" ]; then
    if [ -f /usr/lib/python3/dist-packages/certbot_nginx/_internal/options-ssl-nginx.conf ]; then
      cp /usr/lib/python3/dist-packages/certbot_nginx/_internal/options-ssl-nginx.conf "${SSL_OPTIONS}"
    else
      echo "Using embedded fallback options file"
      printf '%s' "$FALLBACK_SSL_OPTIONS" > "${SSL_OPTIONS}"
    fi
    chmod 644 "${SSL_OPTIONS}"
  fi

  if [ ! -f "${SSL_DH}" ]; then
    if [ -f /usr/lib/python3/dist-packages/certbot/_internal/ssl-dhparams.pem ]; then
      cp /usr/lib/python3/dist-packages/certbot/_internal/ssl-dhparams.pem "${SSL_DH}"
    else
      echo "Generating ${SSL_DH} (could take a minute)"
      openssl dhparam -out "${SSL_DH}" 2048
    fi
    chmod 600 "${SSL_DH}"
  fi
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
  if nginx -t; then
    systemctl reload nginx
  else
    echo "Nginx test failed (likely due to missing certificates). Continuing..."
  fi
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
  test_and_reload_nginx
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
