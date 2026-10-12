# notification-forwarder

Rust Cargo workspace with two binaries:

- `nf-server` — server binary
- `nf-client` — client binary

## Commands

- Build: `cargo build`
- Run: `cargo run -p nf-server` / `cargo run -p nf-client`
- Test: `cargo test`

## nf-server

Tokio TLS TCP server (tokio-rustls) that eavesdrops on the D-Bus session bus (zbus `BecomeMonitor`, matching `org.freedesktop.Notifications.Notify` calls) and pushes each notification to every connected client as a BSON document with fields `summary`, `body`, `icon`, `appname`. BSON documents are self-delimiting, so there is no extra framing.

- `src/notification.rs` — D-Bus monitor, `Notification` type
- `src/tls.rs` — mutual-TLS acceptor built from cert files + `TlsArgs` flags
- `src/certs.rs` — `gen-certs` implementation
- `src/main.rs` — CLI, accept loop, broadcast channel fan-out to clients
- CLI via clap, **no environment variables** (project rule): `--address` (default `0.0.0.0`), `-p/--port` (default 7878), `--log-level` (default info), TLS file flags `--config-dir/--ca/--cert/--key`, subcommand `gen-certs`. On start it logs the listen address and, for `0.0.0.0`, the machine's reachable IPs (`if-addrs`).

## nf-client

Cross-platform (Linux, Windows, macOS). Connects to `nf-server` over TLS (tokio-rustls), reads length-prefixed BSON notification documents and shows each as a desktop notification via `notify-rust`. Platform differences are handled with `cfg`: on Linux the app name and icon are passed through; on Windows/macOS the app name goes in the subtitle and the icon is dropped. The config dir comes from the `dirs` crate. `nf-server` stays Linux-only (D-Bus). Keep client code free of `std::os::unix` outside `cfg(unix)`. Reconnects forever at a fixed interval (`--reconnect-interval`, default 5s).

- CLI via clap, **no environment variables**: `--host` (default `localhost`), `-p/--port` (default 7878), `--log-level`, TLS file flags `--config-dir/--ca/--cert/--key`. The server cert is always verified against the fixed name `nf-server` (`SERVER_NAME` in `nf-client/src/main.rs`), not the connect address.
- Do not run client and server against the same Linux D-Bus session: the client's notifications would be picked up by the server and forwarded again in a loop.
- The server drops identical notifications arriving within 250ms because some desktops (GNOME) relay each `Notify` call through a proxy, which shows up twice on the bus.

## TLS / certificates

Mutual TLS with a private CA. Nothing secret or certificate-related is compiled into the binaries; everything is read from files at startup.

- `nf-server gen-certs [--out DIR] [--days N]` (`nf-server/src/certs.rs`, via rcgen) writes `ca.{pem,key}`, `server.{pem,key}`, `client.{pem,key}` into `DIR` (default `./certs`, gitignored, **never commit**; key files are created 0600; it refuses to overwrite existing files). Certificates contain **no IP/hostname**: the server cert's only SAN is the fixed name `nf-server` (`SERVER_NAME` in `nf-server/src/certs.rs`, must match the client's constant), so the server's address can change freely. Default validity is 825 days.
- Deploy: server host gets `ca.pem`, `server.pem`, `server.key`; each client host gets `ca.pem`, `client.pem`, `client.key`. Never copy `ca.key` anywhere; keep it offline. Keys should be `chmod 600` (the binaries warn otherwise).
- Both binaries look in `--config-dir`, else `notification-forwarder` inside the platform config dir (`dirs::config_dir()`). Individual files can be overridden with `--ca`, `--cert`, `--key`.
- The server only accepts clients whose cert is signed by the CA; the client only trusts servers certified by the CA.

## Conventions

- Every commit message and every PR title must follow [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<optional scope>): <description>`
  - Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`
  - Use `!` or a `BREAKING CHANGE:` footer for breaking changes
  - Scopes are typically the crate name (`server`, `client`)

## Releases

Automated with [release-please](https://github.com/googleapis/release-please) (`.github/workflows/release.yml`, `release-please-config.json`, `.release-please-manifest.json`). It derives the next version and `CHANGELOG.md` from the Conventional Commits on `main` (`feat` → minor, `fix`/`perf` → patch, `!`/`BREAKING CHANGE` → minor while < 1.0; `docs`, `refactor`, `build`, `ci`, `revert` appear in the changelog; `chore`, `style`, `test` are hidden), so **commit messages (or squash-merge PR titles) must be valid Conventional Commits or they won't be released correctly**. Merging the release PR tags `vX.Y.Z`, creates the GitHub release and the same workflow builds and attaches the binaries: `nf-server` + `nf-client` for Linux x86_64, `nf-client` only for Windows x86_64/arm64 and macOS arm64/x86_64 (`nf-server` is Linux-only). Both crates share one version: `[workspace.package] version` in the root `Cargo.toml` (crates use `version.workspace = true`), which release-please updates. `Cargo.lock` is not touched by release-please, so it may need a trivial `cargo build` refresh after a release. The repo setting "Allow GitHub Actions to create and approve pull requests" must be enabled.

## Keeping this file current

Update this file whenever the project changes in a way that affects it (new crates, commands, architecture, conventions).
