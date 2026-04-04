# wasmnet project justfile
# Install just: https://github.com/casey/just
# Get version from Cargo.toml

version := `grep -m 1 'version = ' Cargo.toml | cut -d '"' -f 2`

# Repository information

repo := `git remote get-url origin 2>/dev/null | sed -E 's/.*github.com[:/]([^/]+)\/([^/.]+).*/\1\/\2/' || echo "anistark/wasmnet"`

# Default recipe to display help information
default:
    @just --list
    @echo "\nCurrent version: {{ version }}"

# Build the project
build: sync-version format lint test
    cargo build --release

# Build the npm client package
build-client: sync-version
    cd client && npm run build

# Sync version from Cargo.toml to client/package.json
sync-version:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION="{{ version }}"
    if [ -f "client/package.json" ]; then
        CURRENT=$(grep -m 1 '"version":' client/package.json | cut -d '"' -f 4)
        if [ "$CURRENT" != "$VERSION" ]; then
            if [[ "$OSTYPE" == "darwin"* ]]; then
                sed -i '' "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" client/package.json
            else
                sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" client/package.json
            fi
            echo "✓ Updated client/package.json: $CURRENT → $VERSION"
        else
            echo "✓ client/package.json already at $VERSION"
        fi
    fi

# Clean the project
clean:
    cargo clean
    rm -rf client/dist || true
    find . -name ".DS_Store" -type f -delete || true

# Run the server with default settings
run:
    cargo run

# Run the server on a custom port
run-port PORT="9000":
    cargo run -- --port {{ PORT }}

# Run the server with a policy file
run-policy POLICY="policy.example.toml":
    cargo run -- --policy {{ POLICY }}

# Run the server with no policy restrictions
run-open:
    cargo run -- --no-policy

# Run tests
test:
    cargo test

# Check code formatting
check-format:
    cargo fmt -- --check

# Format code
format:
    cargo fmt

# Run clippy lints
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy with auto-fix
lint-fix:
    cargo clippy --fix --allow-dirty

# Run all checks (lint + test)
check: lint test
    @echo "✅ All checks passed!"

# Build Rust API documentation
docs:
    cargo doc --no-deps --open

# Prepare for publishing (format, lint, test)
prepare-publish: format lint test build
    @echo "✓ Project is ready for publishing"

# Publish to crates.io (requires cargo login)
publish-crates: prepare-publish
    @echo "Publishing version {{ version }} to crates.io..."
    cargo publish --allow-dirty

# Check if you're logged in to npm
check-npm-login:
    @npm whoami > /dev/null 2>&1 || (echo "Not logged in to npm. Run 'npm login' first." && exit 1)
    @echo "Logged in to npm as $(npm whoami)"

# Publish npm client package
publish-npm: build-client check-npm-login
    @echo "Publishing wasmnet v{{ version }} to npm..."
    cd client && npm publish --access public

# Check if you're logged in to crates.io
check-crates-login:
    @if [ -f ~/.cargo/credentials ] || [ -f ~/.cargo/credentials.toml ]; then \
        echo "✓ Logged in to crates.io"; \
    else \
        echo "Not logged in to crates.io. Run 'cargo login' with your token." && exit 1; \
    fi

# Check if you're logged in to GitHub CLI
check-gh-login:
    @gh auth status > /dev/null 2>&1 || (echo "Not logged in to GitHub. Run 'gh auth login' first." && exit 1)
    @echo "✓ Logged in to GitHub"

# Pre-flight checks for all publish targets
pre-publish: check-crates-login check-npm-login check-gh-login
    @echo "✓ All publish credentials verified"

# Install local binary
install:
    cargo install --path .

# Create a new release tag
tag-release:
    git tag v{{ version }}
    @echo "Created tag v{{ version }}"
    echo "Pushing tag v{{ version }} to remote..."
    git push origin "v{{ version }}"

# Create GitHub release
gh-release:
    #!/usr/bin/env bash
    set -euo pipefail

    if ! command -v gh &> /dev/null; then
        echo "Error: GitHub CLI not installed. Please install it from https://cli.github.com/"
        exit 1
    fi

    if ! gh auth status &> /dev/null; then
        echo "Error: Not logged in to GitHub. Please run 'gh auth login'"
        exit 1
    fi

    if ! git rev-parse "v{{ version }}" >/dev/null 2>&1; then
        git tag -a "v{{ version }}" -m "Release v{{ version }}"
        echo "✓ Created tag v{{ version }}"
    else
        echo "✓ Tag v{{ version }} already exists"
    fi

    echo "Pushing tag v{{ version }} to remote..."
    git push origin "v{{ version }}"

    gh release create "v{{ version }}" \
        "./target/release/wasmnet-server"

    echo "✓ GitHub release v{{ version }} created successfully!"
    echo "View it at: https://github.com/{{ repo }}/releases/tag/v{{ version }}"

# Release to GitHub, crates.io, and npm
publish: pre-publish build publish-crates publish-npm gh-release
    @echo "✓ Released v{{ version }} to GitHub, crates.io, and npm"

# Create a pre-release tag with suffix (rc, alpha, beta, etc.)
publish-rc: (publish-tag "rc")

publish-alpha: (publish-tag "alpha")

publish-beta: (publish-tag "beta")

publish-dev: (publish-tag "dev")

# Generic publish with custom tag suffix
publish-tag TAG:
    #!/usr/bin/env bash
    set -euo pipefail

    if ! command -v gh &> /dev/null; then
        echo "Error: GitHub CLI not installed. Please install it from https://cli.github.com/"
        exit 1
    fi

    if ! gh auth status &> /dev/null; then
        echo "Error: Not logged in to GitHub. Please run 'gh auth login'"
        exit 1
    fi

    echo "Building project..."
    cargo build --release

    VERSION_WITH_TAG="{{ version }}-{{ TAG }}"
    TAG_NAME="v$VERSION_WITH_TAG"

    echo "Creating pre-release: $TAG_NAME"

    if git rev-parse "$TAG_NAME" >/dev/null 2>&1; then
        echo "Warning: Tag $TAG_NAME already exists"
        read -p "Do you want to delete and recreate it? (y/N): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            git tag -d "$TAG_NAME" || true
            git push --delete origin "$TAG_NAME" || true
        else
            echo "Cancelled"
            exit 1
        fi
    fi

    echo "Creating tag $TAG_NAME..."
    git tag -a "$TAG_NAME" -m "Pre-release $TAG_NAME"

    echo "Pushing tag $TAG_NAME to remote..."
    git push origin "$TAG_NAME"

    echo "Creating GitHub pre-release..."
    gh release create "$TAG_NAME" \
        --target "$(git rev-parse HEAD)" \
        --title "wasmnet $VERSION_WITH_TAG" \
        --notes "Pre-release version $VERSION_WITH_TAG

    This is a pre-release version for testing and feedback.

    **Installation:**
    \`\`\`bash
    cargo install --git https://github.com/{{ repo }} --tag $TAG_NAME
    \`\`\`

    **Changes since last release:**
    $(git log --oneline $(git describe --tags --abbrev=0 HEAD^)..HEAD | head -10)
    " \
        --prerelease \
        "./target/release/wasmnet-server"

    echo "✓ Pre-release $TAG_NAME created successfully!"
    echo "View it at: https://github.com/{{ repo }}/releases/tag/$TAG_NAME"

# List all available publish commands
publish-help:
    @echo "Available publish commands:"
    @echo "  just publish       - Full release to GitHub and crates.io"
    @echo "  just publish-rc    - Release candidate (v{{ version }}-rc)"
    @echo "  just publish-alpha - Alpha release (v{{ version }}-alpha)"
    @echo "  just publish-beta  - Beta release (v{{ version }}-beta)"
    @echo "  just publish-dev   - Development release (v{{ version }}-dev)"
    @echo "  just publish-tag X - Custom tag release (v{{ version }}-X)"
    @echo ""
    @echo "Current version: {{ version }}"
