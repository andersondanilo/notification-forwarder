# notification-forwarder

Forward desktop notifications from a Linux machine to another machine running **Linux, Windows or macOS**.

- **`nf-server`** runs on the Linux machine whose notifications you want to forward (it needs D-Bus, so it is Linux-only). It listens to the D-Bus session bus for every notification and pushes each one to all connected clients.
- **`nf-client`** is cross-platform and runs on the machine that should display them. It connects to the server and shows each notification as a native desktop notification.

The connection is TCP with **mutual TLS**: the server only accepts clients that hold a certificate signed by your private CA, and clients only trust a server certified by that same CA. Notifications are sent as BSON documents with the fields `summary`, `body`, `icon` and `appname`.

## Requirements

- **Server:** Linux with a D-Bus **session bus**
- **Client:** Linux (with a running notification daemon such as GNOME, KDE, dunst or mako), Windows 10/11, or macOS
- Rust (edition 2024) to build, plus a C toolchain for the TLS library (MSVC Build Tools on Windows, Xcode Command Line Tools on macOS, `gcc`/`clang` on Linux)
- Network access from the client to the server's TCP port (default `7878`; change it with `--port`)

## Install

### From a release (no Rust needed)

Download the archive for your platform from the [Releases page](https://github.com/andersondanilo/notification-forwarder/releases), verify it against the `.sha256` file next to it, and unpack it:

| Platform              | Archive                                              | Contents                |
| --------------------- | ---------------------------------------------------- | ----------------------- |
| Linux x86_64          | `notification-forwarder-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` | `nf-server`, `nf-client` |
| Windows x86_64        | `...-x86_64-pc-windows-msvc.zip`                     | `nf-client.exe`         |
| Windows on ARM        | `...-aarch64-pc-windows-msvc.zip`                    | `nf-client.exe`         |
| macOS Apple Silicon   | `...-aarch64-apple-darwin.tar.gz`                    | `nf-client`             |
| macOS Intel           | `...-x86_64-apple-darwin.tar.gz`                     | `nf-client`             |

The server is Linux-only (it needs D-Bus), so only the Linux archive contains it. The binaries are not code-signed, so macOS Gatekeeper and Windows SmartScreen may warn the first time you run them.

### From source

```sh
git clone https://github.com/andersondanilo/notification-forwarder && cd notification-forwarder
cargo install --path nf-server   # on the (Linux) machine that sends notifications
cargo install --path nf-client   # on the machine that shows them (Linux, Windows or macOS)
```

The binaries are installed to `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). Certificates and keys are **not** compiled in; they are read from files at startup.

## Setup

### 1. Generate certificates

Do this once, on a machine you trust:

```sh
nf-server gen-certs --out ./certs
```

The certificates contain **no IP address or hostname**. The server certificate is issued for the fixed name `nf-server`, and the client always verifies the server against that name, whatever address it connects to. So the server's IP or domain can change at any time without regenerating anything; trust comes from your private CA. Options:

| Option         | Default   | Meaning                                      |
| -------------- | --------- | -------------------------------------------- |
| `--out <DIR>`  | `./certs` | Directory to write the files to              |
| `--days <N>`   | `825`     | Certificate validity                         |

This writes `ca.pem`, `ca.key`, `server.pem`, `server.key`, `client.pem` and `client.key`. It refuses to overwrite existing files, and creates the `.key` files with mode `0600`.

### 2. Distribute the files

Put the files in the `notification-forwarder` folder of the platform config directory on each machine (or pass `--config-dir <DIR>` to either binary to use another folder):

| OS      | Config directory                                          |
| ------- | --------------------------------------------------------- |
| Linux   | `$XDG_CONFIG_HOME/notification-forwarder` or `~/.config/notification-forwarder` |
| macOS   | `~/Library/Application Support/notification-forwarder`    |
| Windows | `%APPDATA%\notification-forwarder`                        |


| Machine | Files needed                          |
| ------- | ------------------------------------- |
| Server  | `ca.pem`, `server.pem`, `server.key`  |
| Client  | `ca.pem`, `client.pem`, `client.key`  |

```sh
# on the Linux server, with the right files copied over
mkdir -p ~/.config/notification-forwarder
cp ca.pem server.pem server.key ~/.config/notification-forwarder/
chmod 600 ~/.config/notification-forwarder/*.key
```

On Linux and macOS clients do the same with `client.pem`/`client.key` (and `chmod 600` the key). On Windows, copy the three client files into `%APPDATA%\notification-forwarder` (restrict the folder to your user, as Windows has no `chmod`).

> **Never copy `ca.key` to the server or clients, and never commit it.** Anyone holding it can issue certificates the server will accept. Keep it offline. All clients can share the same `client.*` files; `gen-certs` does not issue per-client certificates.

## Usage

Start the server on the sending machine:

```sh
nf-server                                  # listens on 0.0.0.0:7878
nf-server --port 9000                      # choose another port
nf-server --address 192.168.1.10 --port 9000   # bind a single address
```

On start it logs the address it listens on and, when listening on all interfaces, the addresses of this machine where clients can reach it, e.g.:

```
listening on 0.0.0.0:7878
reachable at 192.168.1.10:7878
```

Start the client on the receiving machine:

```sh
nf-client --host 192.168.1.10 --port 7878    # Linux / macOS / Windows
```

`--host` takes an IP address or hostname (default `localhost`) and `--port` defaults to `7878`.

Test it from the server machine:

```sh
notify-send "Hello" "It works"
```

If the connection drops, or the server isn't reachable yet, the client keeps retrying forever at a fixed interval (`--reconnect-interval`, default 5 seconds), so you can start it before the server.

> **Do not run the client and the server in the same Linux D-Bus session.** The client's notifications would be picked up by the server and forwarded again, forever.

### Running as a service

**Linux (systemd user units).**

`~/.config/systemd/user/nf-server.service`:

```ini
[Unit]
Description=Notification forwarder server

[Service]
ExecStart=%h/.cargo/bin/nf-server
Restart=on-failure

[Install]
WantedBy=default.target
```

`~/.config/systemd/user/nf-client.service`:

```ini
[Unit]
Description=Notification forwarder client

[Service]
ExecStart=%h/.cargo/bin/nf-client --host 192.168.1.10 --port 7878
Restart=on-failure

[Install]
WantedBy=graphical-session.target
```

Then `systemctl --user enable --now nf-server` (or `nf-client`) on the respective machine.

**Windows / macOS.** There are no service files yet. Start `nf-client` at login yourself: on Windows with a shortcut (or a Task Scheduler "At log on" task) in `shell:startup` whose arguments are `--host <server>`; on macOS with a `launchd` agent in `~/Library/LaunchAgents` whose `ProgramArguments` include `--host <server>`.

## Windows client, step by step

This walks through setting up `nf-client` on Windows 10/11 from scratch. It assumes the server is already running on your Linux machine and you have generated the certificates there (see [Setup](#setup)), and that you know the server's IP address or hostname (below written as `192.168.1.10`). Commands are for **PowerShell**.

> The Windows build has not been tested by the author yet. If a step fails, see [Troubleshooting](#troubleshooting) and please report it.

### 1. Install the build tools

1. Install the **Visual Studio Build Tools** from <https://visualstudio.microsoft.com/visual-cpp-build-tools/>. In the installer select the **"Desktop development with C++"** workload (this provides the MSVC compiler and Windows SDK that Rust and the TLS library need).
2. Install **Rust** from <https://rustup.rs> (download and run `rustup-init.exe`, accept the defaults).
3. Install **Git** from <https://git-scm.com/download/win> if you don't have it.
4. Open a **new** PowerShell window and check:

   ```powershell
   rustc --version
   cargo --version
   git --version
   ```

### 2. Build and install the client

```powershell
git clone https://github.com/andersondanilo/notification-forwarder
cd notification-forwarder
cargo install --path nf-client
```

This installs `nf-client.exe` to `%USERPROFILE%\.cargo\bin`, which the Rust installer already added to your `PATH`. Check that it is found with `Get-Command nf-client`, and see its options with `nf-client --help`.

### 3. Copy the client certificate files from the server

You need exactly three files from the machine where you ran `nf-server gen-certs`: `ca.pem`, `client.pem` and `client.key`. **Do not copy `ca.key`, `server.pem` or `server.key`.**

Create the config folder and copy the files. Windows 10/11 ships with `scp`; replace `user`, the address and the path with your own:

```powershell
$dir = "$env:APPDATA\notification-forwarder"
New-Item -ItemType Directory -Force $dir | Out-Null
scp user@192.168.1.10:/path/to/certs/ca.pem     $dir
scp user@192.168.1.10:/path/to/certs/client.pem $dir
scp user@192.168.1.10:/path/to/certs/client.key $dir
```

(A USB stick or any other secure transfer works too; just end up with those three files in `%APPDATA%\notification-forwarder`.)

### 4. Restrict access to the private key

Windows has no `chmod`, so remove access for other users on the folder. This keeps only your account:

```powershell
icacls $dir /inheritance:r /grant:r "${env:USERNAME}:(OI)(CI)F"
```

### 5. Choose the server address

Nothing to configure ahead of time: you pass the server's address to the client with `--host` and `--port` (default port `7878`). Use the server's IP address or a hostname your Windows machine can resolve. It does not need to appear in the certificates.

### 6. Run it and test

```powershell
nf-client --host 192.168.1.10 --port 7878
```

You should see `connected to 192.168.1.10:7878`. (Add `--log-level debug` for more detail.) Now trigger a notification on the Linux server, for example:

```sh
notify-send "Hello" "From Linux"
```

A Windows toast should appear. It shows the title, the text, and the original app name as a second line. Notifications come from the generic "Windows PowerShell" sender.

If you see no toast, make sure notifications are on (**Settings → System → Notifications**) and that **Do Not Disturb / Focus** is off. If the client prints a certificate error, see [Troubleshooting](#troubleshooting).

Stop the client with `Ctrl+C`.

### 7. Start it automatically at login

Create a shortcut in your Startup folder that runs the client minimized:

```powershell
$lnk = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\nf-client.lnk"
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = "$env:USERPROFILE\.cargo\bin\nf-client.exe"
$s.Arguments = "--host 192.168.1.10 --port 7878"
$s.WindowStyle = 7   # minimized
$s.Save()
```

It will start the next time you sign in. The client keeps retrying if the server is unreachable, so it is fine for it to start before the network is up. To remove autostart, delete that `.lnk` file.

### Updating or uninstalling

- Update: `git pull`, then `cargo install --path nf-client --force`.
- Uninstall: `cargo uninstall nf-client`, delete the Startup shortcut and `%APPDATA%\notification-forwarder`.

## Configuration

Both programs are configured only through command-line options (no environment variables). Run either with `--help` for the full list.

### nf-server

| Option               | Default                         | Meaning                                         |
| -------------------- | ------------------------------- | ----------------------------------------------- |
| `--address <IP>`     | `0.0.0.0`                       | Address to listen on                            |
| `-p`, `--port <N>`   | `7878`                          | Port to listen on                               |
| `--log-level <LVL>`  | `info`                          | `error`, `warn`, `info`, `debug` or `trace`     |
| `--config-dir <DIR>` | platform config dir (see Setup) | Directory holding the TLS files                 |
| `--ca <FILE>`        | `<config dir>/ca.pem`           | CA used to verify clients                       |
| `--cert <FILE>`      | `<config dir>/server.pem`       | Server certificate                              |
| `--key <FILE>`       | `<config dir>/server.key`       | Server private key                              |

Subcommand: `gen-certs [--out <DIR>] [--days <N>]` (see Setup).

### nf-client

| Option               | Default                         | Meaning                                         |
| -------------------- | ------------------------------- | ----------------------------------------------- |
| `--host <HOST>`      | `localhost`                     | Server IP address or hostname                   |
| `-p`, `--port <N>`   | `7878`                          | Server port                                     |
| `--reconnect-interval <SECS>` | `5`                    | Seconds between reconnection attempts (retries forever) |
| `--log-level <LVL>`  | `info`                          | `error`, `warn`, `info`, `debug` or `trace`     |
| `--config-dir <DIR>` | platform config dir (see Setup) | Directory holding the TLS files                 |
| `--ca <FILE>`        | `<config dir>/ca.pem`           | CA used to verify the server                    |
| `--cert <FILE>`      | `<config dir>/client.pem`       | Client certificate                              |
| `--key <FILE>`       | `<config dir>/client.key`       | Client private key                              |

The platform config directory is the one in the Setup table, which comes from the operating system (it is not read from any variable of this project).

## How notifications look on each OS

| Client  | App name | Icon |
| ------- | -------- | ---- |
| Linux   | Shown as the app name (as if sent locally) | Icon names and paths are passed to the notification daemon; a path only works if the file exists on the client |
| Windows | Shown as the toast's second line | Not forwarded |
| macOS   | Shown as the subtitle | Not forwarded |

Only the text (`summary`, `body`) and app name are portable. Notifications on Windows and macOS appear under the OS's generic sender, and macOS may ask you to allow notifications the first time.

## Troubleshooting

- **`invalid peer certificate: ... NotValidForName`** — the server is using a certificate not generated by this version's `gen-certs` (older ones were issued for specific names). Regenerate the certificates and redistribute them.
- **`peer sent no certificates` / `UnknownIssuer`** — the client cert or CA doesn't match the server's CA. Make sure both sides use files from the same `gen-certs` run.
- **`reading .../ca.pem`** — a TLS file is missing; check the config directory.
- **`<file> is accessible by other users`** — run `chmod 600` on your key files (Linux/macOS only; not checked on Windows).
- **`failed to show notification`** — on Linux, no notification daemon is running in the client's session. On Windows/macOS, check that notifications (and Focus / Do Not Disturb) are enabled for the system.
- **Duplicate notifications** — the server already drops identical notifications arriving within 250 ms, because some desktops (e.g. GNOME) relay each notification through a proxy.

## Security notes

- Both the server and the client authenticate each other with certificates; unauthenticated peers are rejected during the TLS handshake.
- Private keys are stored unencrypted on disk and protected only by file permissions.
- Notifications can contain sensitive text; anyone with a valid client certificate receives all of them. Treat `client.key` accordingly.
- The server forwards **every** notification on the user's session bus, from every application.

## Development

```sh
cargo build
cargo test
```

Commits and PR titles follow [Conventional Commits](https://www.conventionalcommits.org/); releases are generated from them by [release-please](https://github.com/googleapis/release-please): merge the release PR it opens to publish a new version with binaries attached. See `CLAUDE.md` for the project layout.
