#!/usr/bin/env bash
# Build SolidCraft for iPad: target/ios/SolidCraft.ipa (and the SolidCraft.app inside it).
#
# Runs on macOS with Xcode installed (the iOS SDK links the binary) and the Rust target added:
#   rustup target add aarch64-apple-ios
#
# Unsigned by default: SideStore, AltStore or Sideloadly sign it with your Apple ID as they install
# it. With a signing identity and a provisioning profile it is signed here instead:
#   SIGN_IDENTITY="Apple Development: you@example.com (TEAMID)" \
#   PROFILE=~/Downloads/SolidCraft.mobileprovision packaging/ios/build-ipa.sh
#
# Options (environment):
#   PROFILE_NAME  Cargo profile: release (default) or dev.
#   BUNDLE_ID     Bundle identifier (default io.tangencylabs.solidcraft.ipad.dev). With a profile,
#                 it must match the profile's App ID.
#   JOBS          Parallel compile jobs (default: Cargo's). Lower it on an 8 GB Mac: JOBS=4.
#   NO_LTO=1      Skip link-time optimisation: a much lighter, faster release link.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

profile="${PROFILE_NAME:-release}"
bundle_id="${BUNDLE_ID:-io.tangencylabs.solidcraft.ipad.dev}"
target=aarch64-apple-ios
out="$root/target/ios"
app="$out/Payload/SolidCraft.app"

[[ "$(uname -s)" == Darwin ]] || { echo "build-ipa.sh: needs macOS with Xcode (the iOS SDK)" >&2; exit 1; }
xcrun --sdk iphoneos --show-sdk-path >/dev/null || { echo "build-ipa.sh: no iOS SDK; install Xcode and run xcode-select" >&2; exit 1; }

cargo_args=(build --locked --target "$target" -p solidcraft-ios --profile "$profile")
[[ -n "${JOBS:-}" ]] && cargo_args+=(-j "$JOBS")
[[ "${NO_LTO:-}" == 1 ]] && export CARGO_PROFILE_RELEASE_LTO=false
# The deployment target the binary is built for, as Info.plist's MinimumOSVersion.
export IPHONEOS_DEPLOYMENT_TARGET=16.0
cargo "${cargo_args[@]}"

dir="$profile"
[[ "$profile" == dev ]] && dir=debug
binary="$root/target/$target/$dir/solidcraft-ios"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
# CFBundleShortVersionString takes up to three numbers: 0.45.0-rc.4 → 0.45.0.
short_version="$(printf '%s' "$version" | sed 's/[^0-9.].*//')"
build="$(git rev-list --count HEAD 2>/dev/null || echo 1)"

rm -rf "$out/Payload" "$out/SolidCraft.ipa"
mkdir -p "$app"
cp "$binary" "$app/solidcraft-ios"
sed -e "s/@BUNDLE_ID@/$bundle_id/" -e "s/@SHORT_VERSION@/$short_version/" -e "s/@BUILD@/$build/" \
  packaging/ios/Info.plist >"$app/Info.plist"
plutil -lint "$app/Info.plist" >/dev/null

# The app icon at the iPad sizes, from the 512 px export. iPadOS masks the corners itself.
icon="assets/icons/app/linux/exports/hicolor/512x512/apps/io.tangencylabs.solidcraft.png"
for spec in "AppIcon76x76@2x~ipad:152" "AppIcon76x76~ipad:76" "AppIcon83.5x83.5@2x~ipad:167"; do
  sips -z "${spec##*:}" "${spec##*:}" "$icon" --out "$app/${spec%%:*}.png" >/dev/null
done
printf 'APPL????' >"$app/PkgInfo"

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  [[ -n "${PROFILE:-}" ]] || { echo "build-ipa.sh: SIGN_IDENTITY needs PROFILE (a .mobileprovision)" >&2; exit 1; }
  cp "$PROFILE" "$app/embedded.mobileprovision"
  entitlements="$out/entitlements.plist"
  security cms -D -i "$PROFILE" >"$out/profile.plist"
  /usr/libexec/PlistBuddy -x -c 'Print :Entitlements' "$out/profile.plist" >"$entitlements"
  codesign --force --sign "$SIGN_IDENTITY" --entitlements "$entitlements" --timestamp=none "$app"
  codesign --verify --strict "$app"
  echo "Signed with $SIGN_IDENTITY"
fi

(cd "$out" && /usr/bin/zip -qry SolidCraft.ipa Payload)
echo "Built $out/SolidCraft.ipa ($(du -h "$out/SolidCraft.ipa" | cut -f1))"
