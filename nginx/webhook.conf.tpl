server {
    listen 443 ssl;
    server_name ${WEBHOOK_DOMAIN};

    ssl_certificate /etc/letsencrypt/live/${WEBHOOK_DOMAIN}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/${WEBHOOK_DOMAIN}/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    location /webhook/facebook {
        proxy_pass http://127.0.0.1:3004/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
