#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(grep '^version' "$REPO_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/' 2>/dev/null || echo "0.0.0")"

if [[ "$VERSION" == *"workspace"* ]]; then
    VERSION="$(grep -A2 '\[workspace.package\]' "$REPO_ROOT/Cargo.toml" | grep 'version' | sed 's/.*"\(.*\)".*/\1/')"
fi

DIST_DIR="$REPO_ROOT/dist"
CLI_DIST="$DIST_DIR/cli"
GUI_DIST="$DIST_DIR/gui"

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Build LoopSmith release binaries.

Options:
  --cli-only      Build only the CLI binary
  --gui-only      Build only the GUI (Tauri) app
  --target TARGET Rust target triple (e.g. x86_64-apple-darwin)
                  Defaults to the host platform.
  --skip-frontend Skip frontend build (use existing dist)
  --sign          Sign macOS bundles with an Apple Developer certificate.
                  Requires APPLE_SIGNING_IDENTITY env var (or uses first
                  available "Developer ID Application" identity).
  --help          Show this help message

Environment variables (for --sign):
  APPLE_SIGNING_IDENTITY   Code signing identity (e.g. "Developer ID Application: Your Name (TEAMID)")
  APPLE_ID                 Apple ID for notarization
  APPLE_ID_PASSWORD        App-specific password for notarization
  APPLE_TEAM_ID            Apple Developer Team ID for notarization

Examples:
  # Build both CLI and GUI for the current platform
  $(basename "$0")

  # Build CLI only for macOS Intel
  $(basename "$0") --cli-only --target x86_64-apple-darwin

  # Build GUI only (Tauri desktop app)
  $(basename "$0") --gui-only

  # Build and sign with Apple Developer certificate
  $(basename "$0") --gui-only --sign
EOF
    exit 0
}

BUILD_CLI=true
BUILD_GUI=true
TARGET=""
SKIP_FRONTEND=false
SIGN_APPLE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --cli-only)   BUILD_CLI=true; BUILD_GUI=false; shift ;;
        --gui-only)   BUILD_CLI=false; BUILD_GUI=true; shift ;;
        --target)     TARGET="$2"; shift 2 ;;
        --skip-frontend) SKIP_FRONTEND=true; shift ;;
        --sign)       SIGN_APPLE=true; shift ;;
        --help)       usage ;;
        *)            echo "Unknown option: $1"; usage ;;
    esac
done

HOST_TARGET="$(rustc -vV | awk '/^host:/ { print $2 }')"
EFFECTIVE_TARGET="${TARGET:-$HOST_TARGET}"

echo "========================================"
echo " LoopSmith Release Build"
echo " Version: $VERSION"
echo " Target:  $EFFECTIVE_TARGET"
echo " CLI:     $BUILD_CLI"
echo " GUI:     $BUILD_GUI"
echo "========================================"

TARGET_FLAG=""
if [[ -n "$TARGET" ]]; then
    TARGET_FLAG="--target $TARGET"
fi

if $BUILD_CLI; then
    echo ""
    echo ">>> Building CLI (loopsmith)..."
    # shellcheck disable=SC2086
    cargo build --release --bin loopsmith $TARGET_FLAG

    if [[ -n "$TARGET" ]]; then
        CLI_BIN="$REPO_ROOT/target/$TARGET/release/loopsmith"
    else
        CLI_BIN="$REPO_ROOT/target/release/loopsmith"
    fi

    mkdir -p "$CLI_DIST"

    ARCHIVE_NAME="loopsmith-${VERSION}-${EFFECTIVE_TARGET}"
    STAGING="$CLI_DIST/$ARCHIVE_NAME"
    rm -rf "$STAGING"
    mkdir -p "$STAGING"

    cp "$CLI_BIN" "$STAGING/"

    case "$EFFECTIVE_TARGET" in
        *-windows-*)
            mv "$STAGING/loopsmith" "$STAGING/loopsmith.exe" 2>/dev/null || true
            ;;
    esac

    (cd "$CLI_DIST" && tar czf "${ARCHIVE_NAME}.tar.gz" "$ARCHIVE_NAME")

    rm -rf "$STAGING"

    echo ">>> CLI archive: $CLI_DIST/${ARCHIVE_NAME}.tar.gz"
    ls -lh "$CLI_DIST/${ARCHIVE_NAME}.tar.gz"
fi

if $BUILD_GUI; then
    echo ""
    echo ">>> Building GUI (LoopSmith Desktop)..."

    GUI_APP_DIR="$REPO_ROOT/apps/harness-ui"

    if ! $SKIP_FRONTEND; then
        echo "    Installing frontend dependencies..."
        (cd "$GUI_APP_DIR" && pnpm install --frozen-lockfile)

        echo "    Building frontend..."
        (cd "$GUI_APP_DIR" && pnpm build)
    fi

    echo "    Building Tauri app..."
    TAURI_TARGET_FLAG=""
    if [[ -n "$TARGET" ]]; then
        TAURI_TARGET_FLAG="--target $TARGET"
    fi
    # shellcheck disable=SC2086
    (cd "$GUI_APP_DIR" && env -u CI pnpm tauri build $TAURI_TARGET_FLAG)

    mkdir -p "$GUI_DIST"

    echo ""
    echo ">>> GUI build complete. Tauri bundles:"
    if [[ -n "$TARGET" ]]; then
        BUNDLE_BASE="$REPO_ROOT/target/$TARGET/release/bundle"
    else
        BUNDLE_BASE="$REPO_ROOT/target/release/bundle"
    fi

    if [[ -d "$BUNDLE_BASE" ]]; then
        if [[ "$EFFECTIVE_TARGET" == *"-apple-"* ]]; then
            APP_BUNDLE=$(find "$BUNDLE_BASE/macos" -maxdepth 1 -name "*.app" -type d | head -1)

            if [[ -n "$APP_BUNDLE" ]]; then
                if $SIGN_APPLE; then
                    IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
                    if [[ -z "$IDENTITY" ]]; then
                        IDENTITY=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed 's/.*"\(.*\)"/\1/')
                    fi

                    if [[ -z "$IDENTITY" ]]; then
                        echo "    ERROR: --sign requested but no signing identity found."
                        echo "    Set APPLE_SIGNING_IDENTITY or install a Developer ID certificate."
                        exit 1
                    fi

                    echo "    Signing .app with identity: $IDENTITY"
                    codesign --force --deep --options runtime --sign "$IDENTITY" "$APP_BUNDLE"
                    echo "    Verifying signature..."
                    codesign --verify --deep --strict "$APP_BUNDLE"
                else
                    echo "    Applying ad-hoc signature to .app..."
                    codesign --force --deep --sign - "$APP_BUNDLE"
                fi
            fi

            DMG_FILE=$(find "$BUNDLE_BASE/dmg" -maxdepth 1 -name "*.dmg" -type f | head -1)

            if [[ -n "${APP_BUNDLE:-}" && -n "${DMG_FILE:-}" ]]; then
                echo "    Rebuilding DMG with signed .app..."
                TEMP_DMG_DIR=$(mktemp -d)
                DMG_NAME=$(basename "$DMG_FILE")
                SIGNED_DMG="$BUNDLE_BASE/dmg/$DMG_NAME"

                cp -R "$APP_BUNDLE" "$TEMP_DMG_DIR/"
                ln -s /Applications "$TEMP_DMG_DIR/Applications"

                rm -f "$SIGNED_DMG"
                hdiutil create -volname "LoopSmith" \
                    -srcfolder "$TEMP_DMG_DIR" \
                    -ov -format UDZO \
                    "$SIGNED_DMG"

                rm -rf "$TEMP_DMG_DIR"

                if $SIGN_APPLE; then
                    echo "    Signing DMG..."
                    codesign --force --sign "$IDENTITY" "$SIGNED_DMG"
                fi
            fi

            if $SIGN_APPLE && [[ -n "${APPLE_ID:-}" && -n "${APPLE_ID_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
                echo "    Submitting DMG for Apple notarization..."
                xcrun notarytool submit "$SIGNED_DMG" \
                    --apple-id "$APPLE_ID" \
                    --password "$APPLE_ID_PASSWORD" \
                    --team-id "$APPLE_TEAM_ID" \
                    --wait

                echo "    Stapling notarization ticket..."
                xcrun stapler staple "$SIGNED_DMG"
            elif $SIGN_APPLE; then
                echo "    NOTICE: Skipping notarization (set APPLE_ID, APPLE_ID_PASSWORD, APPLE_TEAM_ID to enable)"
            fi
        fi

        find "$BUNDLE_BASE" -maxdepth 2 -type f \( -name "*.dmg" -o -name "*.app" -o -name "*.msi" -o -name "*.exe" -o -name "*.deb" -o -name "*.AppImage" -o -name "*.rpm" \) -exec ls -lh {} \;

        find "$BUNDLE_BASE" -maxdepth 2 -type f \( -name "*.dmg" -o -name "*.msi" -o -name "*.deb" -o -name "*.AppImage" -o -name "*.rpm" \) -exec cp {} "$GUI_DIST/" \;

        if [[ -d "$BUNDLE_BASE/macos" ]]; then
            APP_BUNDLE=$(find "$BUNDLE_BASE/macos" -maxdepth 1 -name "*.app" -type d | head -1)
            if [[ -n "$APP_BUNDLE" ]]; then
                (cd "$(dirname "$APP_BUNDLE")" && tar czf "$GUI_DIST/LoopSmith-${VERSION}-${EFFECTIVE_TARGET}.app.tar.gz" "$(basename "$APP_BUNDLE")")
            fi
        fi
    fi

    echo ">>> GUI dist: $GUI_DIST/"
    ls -lh "$GUI_DIST/" 2>/dev/null || echo "    (no bundles found - check build output above)"
fi

echo ""
echo "========================================"
echo " Build complete!"
echo " Output directory: $DIST_DIR"
echo "========================================"
