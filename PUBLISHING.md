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

**Workflow**: `.github/workflows/release.yml` — runs three jobs in sequence:

1. **`build`** — Compiles release binaries in parallel across 3 platform runners:
   - `x86_64-unknown-linux-gnu` on `ubuntu-latest` → `.tar.gz`
   - `x86_64-apple-darwin` on `macos-latest` → `.tar.gz`
   - `aarch64-apple-darwin` on `macos-latest` → `.tar.gz`
   - Each archive contains both `scmux` and `scmux-daemon` binaries
   - Archives are uploaded as build artifacts

2. **`release`** — Collects all build artifacts, generates `checksums.txt` (SHA256), and creates a GitHub Release with auto-generated release notes via `softprops/action-gh-release@v2`.

3. **`publish-crates`** — Publishes both crates to crates.io in dependency order (see [crates.io section](#3-cratesio-automated) below). Uses the `crates-io` GitHub environment for deployment protection.

**How to trigger**:
```bash
git tag v0.2.0
git push origin v0.2.0
```

### 2. Homebrew Tap (Manual)

**Repository**: [`randlee/homebrew-tap`](https://github.com/randlee/homebrew-tap)
**Formula**: `Formula/scmux.rb`

**Update process after a new GitHub Release**:

1. Wait for the GitHub Release workflow to complete
2. Download `checksums.txt` from the release assets
3. Update `Formula/scmux.rb` in the homebrew-tap repo:
   - Update `version` to match the new release
   - Update SHA256 hashes for each platform from `checksums.txt`
   - Update download URLs to point to the new release tag
4. Commit and push to `randlee/homebrew-tap`

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
3. Add the token as a GitHub repository secret named `CARGO_REGISTRY_TOKEN`:
   - Go to https://github.com/randlee/scmux/settings/secrets/actions
   - Click "New repository secret"
   - Name: `CARGO_REGISTRY_TOKEN`, Value: your crates.io token
4. Create a GitHub environment named `crates-io`:
   - Go to https://github.com/randlee/scmux/settings/environments
   - Click "New environment", name it `crates-io`
   - Optionally add protection rules (e.g., required reviewers)

**What happens**:
- The `publish-crates` job in `.github/workflows/release.yml` runs after the GitHub Release is created
- Publishes `scmux-daemon` first, waits 60s for crates.io indexing, then publishes `scmux`
- Uses the `crates-io` environment for deployment protection

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

1. **Tag the release**:
   ```bash
   git checkout main
   git pull origin main
   git tag v0.2.0
   git push origin v0.2.0
   ```

2. **Monitor GitHub Actions**: Watch the Release workflow at https://github.com/randlee/scmux/actions

3. **Verify the release**: Check https://github.com/randlee/scmux/releases for:
   - 3 platform archives (linux x86_64, macOS x86_64, macOS arm64)
   - `checksums.txt`
   - Auto-generated release notes

### After Release

4. **Verify crates.io publish**: The `publish-crates` job runs automatically after the GitHub Release is created. Check the Actions tab for status. If it fails, use the manual fallback commands in the crates.io section above.

5. **Update Homebrew tap**:
   - Get SHA256s from `checksums.txt`
   - Update `Formula/scmux.rb` in `randlee/homebrew-tap`

6. **Announce**: Update any relevant documentation or channels

---

## Version Strategy

Version numbers follow semantic versioning. Each minor version corresponds to a significant feature milestone.

| Version | Milestone |
|---------|-----------|
| 0.1.0 | Initial release — core tmux session management |
| 0.2.0 | Next milestone (planned) |
| 1.0.0 | Stable release (TBD) |

---

## One-Time Setup

### GitHub Repository Secrets

| Secret | Description |
|--------|-------------|
| `CARGO_REGISTRY_TOKEN` | crates.io API token with publish scope |

### GitHub Environments

| Environment | Purpose |
|-------------|---------|
| `crates-io` | Gate for crates.io publish job; add required reviewers if desired |

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
    on_intel do
      url "https://github.com/randlee/scmux/releases/download/v#{version}/scmux-v#{version}-x86_64-apple-darwin.tar.gz"
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
