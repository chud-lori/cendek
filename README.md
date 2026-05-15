# cendek

Tiny Rust URL shortener.

It is intentionally dependency-free:

- one native binary
- one `links.tsv` file
- in-memory hash lookup
- direct `302` redirects
- no database process
- no framework/runtime

## Links

Edit `links.tsv`:

```text
app	https://app.example.com/
docs	https://docs.example.com/
repo	https://git.example.com/project
```

Format is:

```text
slug<TAB>target_url
```

Allowed target prefixes are `https://`, `http://`, and `mailto:`.

## Run

```sh
cargo run --release -- --addr 127.0.0.1:8080 --links links.tsv
```

Then:

```sh
curl -I http://127.0.0.1:8080/m
```

## Build

```sh
cargo build --release
```

The binary is:

```text
target/release/cendek
```

Deploy only the binary and `links.tsv` to the server.

## Routes

```text
/           homepage with available links
/<slug>     302 redirect
/api/links  JSON list
/healthz    health check
/robots.txt robots file
```

## Deployment

See `DEPLOYMENT.md` for an nginx and systemd setup.
