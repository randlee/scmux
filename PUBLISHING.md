# Publishing Guide

Complete publishing workflow for all distribution channels.

## Package Listings

Published packages and where to find them:

| Channel | URL |
|---------|-----|
| **GitHub Releases** | <https://github.com/randlee/scmux/releases> |
| **Homebrew Tap** | <https://github.com/randlee/homebrew-tap> |
| **crates.io** — `scmux-daemon` | <https://crates.io/crates/scmux-daemon> |
| **crates.io** — `scmux` (CLI) | <https://crates.io/crates/scmux> |
| **Release workflow runs** | <https://github.com/randlee/scmux/actions/workflows/release.yml> |

---

## Distribution Channels

### 1. GitHub Releases (Automated)

**Trigger**: Push a tag matching `v*` (e.g., `v0.2.0`).

**Workflow**: `.github/workflows/release.yml` — runs two jobs in sequence:

1. **`build`** — Compiles release binaries in parallel across 3 platform runners:
   - `x86_64-unknown-linux-gnu` on `ubuntu-latest` → `.tar.gz`
   - `aarch64-apple-darwin` on `macos-latest` → `.tar.gz`
   - `x86_64-pc-windows-msvc` on `windows-latest` → `.zip`
   - Each archive contains both `scmux` and `scmux-daemon` binaries
   - Archives are uploaded as build artifacts

2. **`release`** — Collects all build artifacts, generates `checksums.txt` (SHA256), creates a GitHub Release with auto-generated release notes via `softprops/action-gh-release@v2`, then publishes crates to crates.io in dependency order (see [crates.io section](#3-cratesio-automated) below).

**How to trigger**:
```bash
git tag v0.2.0
git push origin v0.2.0
```

### 2. Homebrew Tap (Automated)

**Repository**: [`randlee/homebrew-tap`](https://github.com/randlee/homebrew-tap)
**Formula**: `Formula/scmux.rb`

Homebrew updates are handled by the `update-homebrew` job in
`.github/workflows/release.yml` after the `release` job completes.

The job:
1. Downloads `checksums.txt` from the new GitHub release
2. Parses SHA256 values for:
   - `aarch64-apple-darwin`
   - `x86_64-unknown-linux-gnu`
3. Checks out `randlee/homebrew-tap` using `HOMEBREW_TAP_TOKEN`
4. Patches `Formula/scmux.rb` (version, URLs, sha256 values) and pushes

Note: Homebrew formula coverage intentionally includes only `aarch64-apple-darwin`
and `x86_64-unknown-linux-gnu`. Intel macOS (`x86_64-apple-darwin`) is not part of
the current release artifact matrix.

**Verification**:
```bash
brew update
brew upgrade scmux
# or for fresh install:
brew tap randlee/tap
brew install scmux
```

### 3. crates.io (Automated)

**Trigger**: Runs automatically as part of the release workflow after the GitHub Release is created.

**Crates published** (in dependency order, with 60s indexing delay between each):
1. `scmux-daemon` — daemon binary
2. `scmux` — CLI binary

**Setup** (one-time):
1. Create a crates.io account at https://crates.io (login with GitHub)
2. Generate an API token at https://crates.io/settings/tokens with publish scope
3. Add the token as a GitHub repository secret named `CRATES_IO_TOKEN`:
   - Go to https://github.com/randlee/scmux/settings/secrets/actions
   - Click "New repository secret"
   - Name: `CRATES_IO_TOKEN`, Value: your crates.io token

**What happens**:
- The `release` job in `.github/workflows/release.yml` publishes crates immediately after the GitHub Release is created
- Publishes `scmux-daemon` first, waits 60 seconds for crates.io indexing, then publishes `scmux`

**Cargo.toml metadata**: All required fields (`description`, `license`, `repository`, `keywords`, `categories`) are present in workspace and crate configs.

**Manual publishing** (fallback if automated publish fails):
```bash
cargo login <your-crates-io-token>
cargo publish -p scmux-daemon
# Wait ~60s for crates.io indexing
cargo publish -p scmux
```

---

## Release Checklist

### Before Release

- [ ] All tests pass: `cargo test --workspace`
- [ ] Clippy clean: `cargo clippy --workspace -- -D warnings`
- [ ] Version bumped in workspace `Cargo.toml` (`[workspace.package] version`)
- [ ] CHANGELOG or release notes drafted (optional — GitHub auto-generates from PRs)
- [ ] All changes merged to `main` via PR from `develop`

### Release

1. **Spawn publisher** and wait for pre-publish audit report.

2. **Push the tag** (team-lead only, after publisher confirms audit pass):
   ```bash
   git tag vX.Y.Z origin/main
   git push origin vX.Y.Z
   ```

3. **Monitor GitHub Actions**: publisher monitors the Release workflow using background agents.

3. **Verify the release**: Check https://github.com/randlee/scmux/releases for:
   - 3 platform archives (linux x86_64, macOS arm64, windows x86_64)
   - `checksums.txt`
   - Auto-generated release notes

### After Release

4. **Verify crates.io publish**: The `release` job publishes crates automatically after the GitHub Release is created. Check the Actions tab for status. If it fails, use the manual fallback commands in the crates.io section above.

5. **Verify Homebrew auto-update**:
   - Confirm `update-homebrew` job passed in the release workflow
   - Confirm `randlee/homebrew-tap/Formula/scmux.rb` was updated

6. **Announce**: Update any relevant documentation or channels

---

## Version Strategy

Version numbers follow semantic versioning. Each minor version corresponds to a significant feature milestone.

| Version | Milestone |
|---------|-----------|
| 0.1.0 | Initial release — core tmux session management |
| 0.2.0 | Multi-host, dashboard, CI polling, CLI binary |
| 0.3.0 | Supervision, /health, launchd/systemd, E2E tests |
| 0.4.1 | Dashboard embed, crates.io publish, Homebrew, ATM integration |
| 0.5.0 | Definition-first architecture, CLI write commands, scmux doctor |
| 1.0.0 | Stable release (TBD) |

## Premature-Tag Recovery

If a `v*` tag was pushed before a successful final release from `main`:

1. Mark that version burned — do not reuse, move, or delete the tag.
2. Bump patch version on `develop` (`X.Y.Z → X.Y.(Z+1)`).
3. Run the full release flow with the new version.

This applies to accidental tags, failed release workflows, and any case where the tag exists before the release workflow completed successfully.

---

## One-Time Setup

### GitHub Repository Secrets

| Secret | Description |
|--------|-------------|
| `CRATES_IO_TOKEN` | crates.io API token with publish scope |
| `HOMEBREW_TAP_TOKEN` | fine-grained PAT with write access to `randlee/homebrew-tap` |

### Homebrew Tap

Create the `randlee/homebrew-tap` repository and add a formula at `Formula/scmux.rb`. Template:

```ruby
class Scmux < Formula
  desc "tmux session manager CLI for multi-agent Claude Code teams"
  homepage "https://github.com/randlee/scmux"
  version "0.1.0"

  on_macos do
    on_arm do
      url "https://github.com/randlee/scmux/releases/download/v#{version}/scmux-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "<sha256-from-checksums.txt>"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/randlee/scmux/releases/download/v#{version}/scmux-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "<sha256-from-checksums.txt>"
    end
  end

  def install
    bin.install "scmux"
    bin.install "scmux-daemon"
  end

  test do
    system "#{bin}/scmux", "--version"
  end
end
```
