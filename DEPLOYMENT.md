# Deployment

This guide deploys `cendek` behind nginx on a Linux server.

The examples use generic placeholders:

- domain: `cendek.example.com`
- app user: `cendek`
- install directory: `/opt/cendek`
- local app address: `127.0.0.1:1234`

Replace those values with your own.

## 1. Choose a Build Method

If your local machine and server use different operating systems, do not upload the normal local build.

For example:

```text
Apple Silicon macOS build  -> runs on macOS only
Ubuntu server              -> needs a Linux binary
```

Use one of these methods. In all cases, create the service user first.

## 2. Create Service User

Create the service user and install directory:

```sh
sudo useradd --system --home /opt/cendek --shell /usr/sbin/nologin cendek
sudo mkdir -p /opt/cendek
sudo chown -R cendek:cendek /opt/cendek
```

If the user already exists, that is fine. Continue with the remaining commands.

## 3. Option A: Download a GitHub Release Binary

This is the smallest server setup. The server needs only `curl`, `tar`, and systemd/nginx.

Create a release from your local machine:

```sh
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions will build the Linux x86_64 binary and attach release assets.

On the server, download the latest release archive:

```sh
REPO="chud-lori/cendek"
VERSION="v0.1.0"
ASSET="cendek-x86_64-unknown-linux-musl.tar.gz"

curl -fL "https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}" -o /tmp/cendek.tar.gz
curl -fL "https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}.sha256" -o /tmp/cendek.tar.gz.sha256
cd /tmp && sha256sum -c cendek.tar.gz.sha256
tar -xzf /tmp/cendek.tar.gz -C /tmp
```

Or download the latest release without pinning a version:

```sh
REPO="chud-lori/cendek"
ASSET="cendek-x86_64-unknown-linux-musl.tar.gz"

curl -fL "https://github.com/${REPO}/releases/latest/download/${ASSET}" -o /tmp/cendek.tar.gz
curl -fL "https://github.com/${REPO}/releases/latest/download/${ASSET}.sha256" -o /tmp/cendek.tar.gz.sha256
cd /tmp && sha256sum -c cendek.tar.gz.sha256
tar -xzf /tmp/cendek.tar.gz -C /tmp
```

Install it:

```sh
sudo mv /tmp/cendek /opt/cendek/cendek
sudo test -f /opt/cendek/links.tsv || sudo mv /tmp/links.example.tsv /opt/cendek/links.tsv
sudo chown -R cendek:cendek /opt/cendek
sudo chmod 755 /opt/cendek/cendek
```

The release archive includes `links.example.tsv` only. Your real `/opt/cendek/links.tsv` stays on the server and is not overwritten by later deploys.

If the repository is private, use a GitHub token with release read access:

```sh
curl -fL \
  -H "Authorization: Bearer $GITHUB_TOKEN" \
  "https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}" \
  -o /tmp/cendek.tar.gz
```

## 4. Option B: Build on the Ubuntu Server

Build on the server:

```sh
git clone <repo-url> /opt/cendek-src
cd /opt/cendek-src
cargo build --release
```

Then install:

```sh
sudo mkdir -p /opt/cendek
sudo cp target/release/cendek /opt/cendek/cendek
sudo cp links.tsv /opt/cendek/links.tsv
sudo chown -R cendek:cendek /opt/cendek
sudo chmod 755 /opt/cendek/cendek
```

This is simple, but the server needs Rust, Cargo, and git.

## 5. Option C: Cross-Compile Locally, Upload Binary Only

This keeps the server smaller because it does not need Rust or source code.

Install `cross` locally:

```sh
cargo install cross
```

For a normal Intel/AMD Ubuntu VPS:

```sh
cross build --release --target x86_64-unknown-linux-musl
```

For an ARM Ubuntu server:

```sh
cross build --release --target aarch64-unknown-linux-musl
```

Upload only the binary and link file:

```text
target/x86_64-unknown-linux-musl/release/cendek
links.tsv
```

Example upload:

```sh
scp target/x86_64-unknown-linux-musl/release/cendek links.tsv user@server:/tmp/
```

Install on the server:

```sh
sudo mkdir -p /opt/cendek
sudo mv /tmp/cendek /opt/cendek/cendek
sudo mv /tmp/links.tsv /opt/cendek/links.tsv
sudo chown -R cendek:cendek /opt/cendek
sudo chmod 755 /opt/cendek/cendek
```

Use the `aarch64-unknown-linux-musl` path instead if your server is ARM:

```text
target/aarch64-unknown-linux-musl/release/cendek
```

## 6. systemd

Create `/etc/systemd/system/cendek.service`:

```sh
sudo tee /etc/systemd/system/cendek.service >/dev/null <<'EOF'
[Unit]
Description=cendek URL shortener
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=cendek
Group=cendek
WorkingDirectory=/opt/cendek
ExecStart=/opt/cendek/cendek --addr 127.0.0.1:1234 --links /opt/cendek/links.tsv
Restart=on-failure
RestartSec=2

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/cendek

[Install]
WantedBy=multi-user.target
EOF
```

Enable and start:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now cendek
sudo systemctl status cendek
```

Check the app locally:

```sh
curl -I http://127.0.0.1:1234/healthz
```

## 7. nginx

Create `/etc/nginx/sites-available/cendek`:

```nginx
server {
    listen 80;
    server_name cendek.example.com;

    location / {
        proxy_pass http://127.0.0.1:1234;
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

## 8. HTTPS

If you use Certbot:

```sh
sudo certbot --nginx -d cendek.example.com
```

If TLS is terminated by a CDN or load balancer, configure HTTPS there and keep nginx bound to the local server as needed.

## 9. Add or Change Links

Edit `/opt/cendek/links.tsv` on the server:

```sh
sudo nano /opt/cendek/links.tsv
```

Each line uses this format:

```text
slug<TAB>target_url
```

Example:

```text
app	https://app.example.com/
docs	https://docs.example.com/
repo	https://git.example.com/project
```

The separator must be a real tab. Spaces are not accepted.

After changing links, restart the service because `cendek` loads `links.tsv` at startup:

```sh
sudo systemctl restart cendek
```

Verify one short link:

```sh
curl -I http://127.0.0.1:1234/app
```

Expected result:

```text
HTTP/1.1 302 Found
location: https://app.example.com/
```

## 10. Verify

```sh
curl -I https://cendek.example.com/app
curl https://cendek.example.com/api/links
curl https://cendek.example.com/healthz
```

`/api/links` returns both the short URL and destination URL:

```json
[
  {
    "slug": "app",
    "url": "https://cendek.example.com/app",
    "target": "https://app.example.com/"
  }
]
```
