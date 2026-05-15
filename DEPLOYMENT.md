# Deployment

This guide deploys `cendek` behind nginx on a Linux server.

The examples use generic placeholders:

- domain: `short.example.com`
- app user: `cendek`
- install directory: `/opt/cendek`
- local app address: `127.0.0.1:8080`

Replace those values with your own.

## 1. Build

Build on the server:

```sh
git clone <repo-url> /opt/cendek-src
cd /opt/cendek-src
cargo build --release
```

Or build locally and upload only:

```text
target/release/cendek
links.tsv
```

## 2. Install Files

Create a service user and install directory:

```sh
sudo useradd --system --home /opt/cendek --shell /usr/sbin/nologin cendek
sudo mkdir -p /opt/cendek
sudo cp target/release/cendek /opt/cendek/cendek
sudo cp links.tsv /opt/cendek/links.tsv
sudo chown -R cendek:cendek /opt/cendek
sudo chmod 755 /opt/cendek/cendek
```

## 3. systemd

Create `/etc/systemd/system/cendek.service`:

```ini
[Unit]
Description=cendek URL shortener
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=cendek
Group=cendek
WorkingDirectory=/opt/cendek
ExecStart=/opt/cendek/cendek --addr 127.0.0.1:8080 --links /opt/cendek/links.tsv
Restart=on-failure
RestartSec=2

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/cendek

[Install]
WantedBy=multi-user.target
```

Enable and start:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now cendek
sudo systemctl status cendek
```

Check the app locally:

```sh
curl -I http://127.0.0.1:8080/healthz
```

## 4. nginx

Create `/etc/nginx/sites-available/cendek`:

```nginx
server {
    listen 80;
    server_name short.example.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Enable it:

```sh
sudo ln -s /etc/nginx/sites-available/cendek /etc/nginx/sites-enabled/cendek
sudo nginx -t
sudo systemctl reload nginx
```

## 5. HTTPS

If you use Certbot:

```sh
sudo certbot --nginx -d short.example.com
```

If TLS is terminated by a CDN or load balancer, configure HTTPS there and keep nginx bound to the local server as needed.

## 6. Add Links

Edit `/opt/cendek/links.tsv`:

```text
app	https://app.example.com/
docs	https://docs.example.com/
```

Restart the service:

```sh
sudo systemctl restart cendek
```

## 7. Verify

```sh
curl -I https://short.example.com/app
curl https://short.example.com/api/links
curl https://short.example.com/healthz
```
