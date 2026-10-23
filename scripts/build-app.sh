#!/usr/bin/env bash
# Builds build/Dictator.app: release binary, the unsandboxed updater, llama.framework (for the
# optional S1-mini cleanup, ADR 0011) and the pinned model manifest, signed with hardened runtime.
# The model itself is not bundled; the app downloads and verifies it on first launch (ADR 0004).
#   SIGN_IDENTITY="Developer ID Application: …" scripts/build-app.sh   (default: ad-hoc "-")
#     With a real identity the signature gets a secure timestamp, as notarization requires.
#   VERSION=1.2.3 scripts/build-app.sh   (overrides CFBundleShortVersionString; default: Info.plist)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/build/Dictator.app"
identity="${SIGN_IDENTITY:--}"

swift build --package-path "$root" -c release --arch arm64
bin="$(swift build --package-path "$root" -c release --arch arm64 --show-bin-path)"

rm -rf "$app"
xpc="$app/Contents/XPCServices/DictatorUpdater.xpc"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$xpc/Contents/MacOS"
cp "$bin/Dictator" "$app/Contents/MacOS/Dictator"
cp "$bin/DictatorUpdater" "$xpc/Contents/MacOS/DictatorUpdater"
cp "$root/Resources/DictatorUpdater.plist" "$xpc/Contents/Info.plist"
cp "$root/Resources/Info.plist" "$app/Contents/Info.plist"
if [[ -n "${VERSION:-}" ]]; then
    /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$app/Contents/Info.plist"
fi
# Shown in the What's New window; bundled so it works offline.
cp "$root/CHANGELOG.md" "$app/Contents/Resources/CHANGELOG.md"
cp "$root/LICENSE" "$app/Contents/Resources/LICENSE"
cp "$root/THIRD_PARTY_NOTICES.md" "$app/Contents/Resources/THIRD_PARTY_NOTICES.md"
# The S1-mini licence (with its naming term), its NOTICE, and llama.cpp's licence, as they require.
mkdir -p "$app/Contents/Resources/ThirdParty"
cp "$root/Resources/ThirdParty/"* "$app/Contents/Resources/ThirdParty/"
iconutil --convert icns "$root/Resources/AppIcon.iconset" --output "$app/Contents/Resources/AppIcon.icns"
# The manifest is the trust anchor for the download: sealed by the app's signature, never fetched.
cp "$root/scripts/model-manifest.txt" "$app/Contents/Resources/model-manifest.txt"

# llama.cpp (MIT, official xcframework) is a dynamic framework: embed it, keep only arm64, and let
# the executable find it in Contents/Frameworks.
framework="$bin/llama.framework"
if [[ ! -d "$framework" ]]; then
    framework="$(find "$root/.build" -path '*macos*' -name llama.framework -type d -prune | head -n 1)"
fi
if [[ -z "$framework" || ! -d "$framework" ]]; then
    echo "llama.framework not found in the build output" >&2
    exit 1
fi
mkdir -p "$app/Contents/Frameworks"
ditto "$framework" "$app/Contents/Frameworks/llama.framework"
llama_bin="$app/Contents/Frameworks/llama.framework/Versions/Current/llama"
if lipo -archs "$llama_bin" | grep -q x86_64; then
    lipo "$llama_bin" -thin arm64 -output "$llama_bin.arm64"
    mv "$llama_bin.arm64" "$llama_bin"
fi
if ! otool -l "$app/Contents/MacOS/Dictator" | grep -q '@executable_path/../Frameworks'; then
    install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/Contents/MacOS/Dictator"
fi

# Sign inside out. The updater XPC is unsandboxed (no entitlements) so it can replace the app;
# the app itself stays sandboxed. --deep is not used.
if [[ "$identity" == "-" ]]; then timestamp="--timestamp=none"; else timestamp="--timestamp"; fi
codesign --force --options runtime "$timestamp" --sign "$identity" "$xpc"
codesign --force --options runtime "$timestamp" --sign "$identity" "$app/Contents/Frameworks/llama.framework"
codesign --force --options runtime "$timestamp" \
    --entitlements "$root/Resources/Dictator.entitlements" \
    --sign "$identity" "$app"
codesign --verify --strict --deep "$app"
codesign --verify --strict "$xpc"
codesign --verify --strict "$app/Contents/Frameworks/llama.framework"
app_entitlements="$(codesign -d --entitlements - "$app" 2>/dev/null || true)"
xpc_entitlements="$(codesign -d --entitlements - "$xpc" 2>/dev/null || true)"
xpc_identity="$(codesign -d -v "$xpc" 2>&1 || true)"
if ! grep -q 'com.apple.security.app-sandbox' <<<"$app_entitlements"; then
    echo "Dictator.app must stay sandboxed" >&2
    exit 1
fi
if grep -q 'com.apple.security.app-sandbox' <<<"$xpc_entitlements"; then
    echo "DictatorUpdater.xpc must not be sandboxed" >&2
    exit 1
fi
if ! grep -q 'Identifier=com.captainduckay.dictator.updater' <<<"$xpc_identity"; then
    echo "DictatorUpdater.xpc has the wrong identifier" >&2
    exit 1
fi

echo "✓ Built $app"
