# Deployment

This repo ships service templates for auto-start and crash-restart behavior:

- macOS launchd: `deploy/macos/com.scmux.scmux-daemon.plist`
- Linux systemd: `deploy/linux/scmux-daemon.service`

Both templates assume the daemon binary is installed at `/usr/local/bin/scmux-daemon`. Update the path if your install location differs.

## macOS (launchd)

1. Build/install the binary:
```bash
cargo build --release --package scmux-daemon
install -m 0755 target/release/scmux-daemon /usr/local/bin/scmux-daemon
```

2. Install the launch agent:
```bash
mkdir -p ~/Library/LaunchAgents
cp deploy/macos/com.scmux.scmux-daemon.plist ~/Library/LaunchAgents/
launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/com.scmux.scmux-daemon.plist
launchctl enable "gui/$(id -u)/com.scmux.scmux-daemon"
launchctl kickstart -k "gui/$(id -u)/com.scmux.scmux-daemon"
```

3. Verify:
```bash
launchctl print "gui/$(id -u)/com.scmux.scmux-daemon" | head -40
curl -s http://localhost:7878/health
```

4. Uninstall:
```bash
launchctl bootout "gui/$(id -u)" ~/Library/LaunchAgents/com.scmux.scmux-daemon.plist
rm -f ~/Library/LaunchAgents/com.scmux.scmux-daemon.plist
```

## Linux (systemd)

1. Build/install the binary:
```bash
cargo build --release --package scmux-daemon
sudo install -m 0755 target/release/scmux-daemon /usr/local/bin/scmux-daemon
```

2. Install and start the service:
```bash
sudo cp deploy/linux/scmux-daemon.service /etc/systemd/system/scmux-daemon.service
sudo systemctl daemon-reload
sudo systemctl enable --now scmux-daemon
```

3. Verify:
```bash
systemctl status scmux-daemon --no-pager
curl -s http://localhost:7878/health
```

4. Uninstall:
```bash
sudo systemctl disable --now scmux-daemon
sudo rm -f /etc/systemd/system/scmux-daemon.service
sudo systemctl daemon-reload
```

## Notes

- `Restart=always` (systemd) and `KeepAlive=true` (launchd) satisfy crash-restart behavior.
- Boot auto-start is provided by `enable --now` (systemd) and `RunAtLoad` + `bootstrap` (launchd).
- Use `SCMUX_LOG` to tune daemon log verbosity in either service definition.
