#!/bin/bash
# The Mac app for direct download (README.md "Mac app (direct download)"), outside the App Store and TestFlight:
# built on a macOS runner from the commit of a release, signed with the team's "Developer ID Application" certificate
# and the hardened runtime, notarized by Apple and stapled, packed as Trommi-macOS.dmg (signed, notarized and stapled
# as well) with its SHA-256. mac.yml runs it under `op run`; attaching it to the release is mac_release.sh's (Linux).
#   mac_direct.sh build    CI: everything above; writes ready (yes|no), version and build to GITHUB_OUTPUT
#   mac_direct.sh adhoc    on any Mac without secrets: the same archive, signed ad hoc, as a .dmg, with the checks
#                          that hold for an ad hoc signature (Gatekeeper must refuse it); nothing is sent anywhere
#
# Fails closed: without the Developer ID certificate or the App Store Connect key (notarytool) the build mode ends
# green with a notice (ready=no) and builds nothing; it never ships an app that is not Developer ID signed, notarized
# and stapled, and any failed check ends it red.
#
# Steps of `build`: the certificate (.p12) into a keychain of its own under RUNNER_TEMP (removed at the end, the
# keychain search list put back) -> Trommi.xcodeproj (XcodeGen) -> archive, signed ad hoc as in mac_testflight.sh
# (TrommiMac.entitlements, hardened runtime) -> export with method developer-id: Xcode signs with that certificate and
# makes or renews the Developer ID profile (push, associated domains) through the App Store Connect key -> checks:
# Developer ID authority of APPLE_TEAM_ID, the very certificate of the .p12, hardened runtime, a secure timestamp, a
# Developer ID profile, the entitlements of TrommiMac.entitlements, no get-task-allow -> notarytool (the same key),
# staple, Gatekeeper -> .dmg with the app and a link to /Applications, signed, notarized, stapled, checked again.
#
# Environment: RUNNER_TEMP, COMMIT (the checkout), VERSION (the release number: the app's build number), XCODEGEN;
# build only, from the 1Password Environment: APPLE_ASC_KEY, APPLE_ASC_KEY_ID, APPLE_ASC_ISSUER_ID, APPLE_TEAM_ID
# (as for TestFlight), APPLE_DEVID_P12 (the .p12 of the Developer ID Application certificate with its private key, in
# base64), APPLE_DEVID_P12_PASSWORD. MAC_OUT: where the files go (default $RUNNER_TEMP/mac-direct). The Rust core
# for the Mac must be in place (core/swift/TrommiCoreRust/lib/macos). Nothing here prints a value of a secret.
set -euo pipefail

mode=${1:-build}
case "$mode" in build|adhoc) ;; *) echo "usage: mac_direct.sh build|adhoc" >&2; exit 2 ;; esac

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
appstore=$repo/app/ios/TrommiApp/AppStore
name=Trommi-macOS
out() { [ -z "${GITHUB_OUTPUT:-}" ] || echo "$1=$2" >> "$GITHUB_OUTPUT"; }
summary() { [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "$1" >> "$GITHUB_STEP_SUMMARY"; }
out ready no

if [ "$mode" = adhoc ]; then
  RUNNER_TEMP=${RUNNER_TEMP:-$(mktemp -d "${TMPDIR:-/tmp}/mac-direct.XXXXXX")}
  COMMIT=${COMMIT:-$(git -C "$repo" rev-parse HEAD)}
  VERSION=${VERSION:-1}
  XCODEGEN=${XCODEGEN:-$(command -v xcodegen || echo "$HOME/bin/xcodegen")}
fi
for var in RUNNER_TEMP COMMIT VERSION XCODEGEN; do
  [ -n "${!var:-}" ] || { echo "::error::$var is not set for this run"; exit 1; }
done
printf '%s' "$VERSION" | grep -Eq '^[1-9][0-9]{0,11}$' || { echo "::error::not a release number: $VERSION"; exit 1; }
printf '%s' "$COMMIT" | grep -Eq '^[0-9a-f]{40}$' || { echo "::error::not a commit: $COMMIT"; exit 1; }
[ "$mode" = adhoc ] || [ "$(git -C "$repo" rev-parse HEAD)" = "$COMMIT" ] || { echo "::error::the checkout is not $COMMIT"; exit 1; }

if [ "$mode" = build ]; then
  missing=
  for var in APPLE_DEVID_P12 APPLE_DEVID_P12_PASSWORD APPLE_ASC_KEY APPLE_ASC_KEY_ID APPLE_ASC_ISSUER_ID APPLE_TEAM_ID; do
    [ -n "${!var:-}" ] || missing="$missing $var"
  done
  if [ -n "$missing" ]; then
    echo "::notice::the Mac app for direct download is not built: missing in the 1Password Environment:$missing (README.md \"Mac app (direct download)\")"
    summary "### Mac (direct download): not built, signing is not set up (missing:$missing)"
    exit 0
  fi
  printf '%s' "$APPLE_TEAM_ID" | grep -Eq '^[A-Z0-9]{10}$' || { echo "::error::APPLE_TEAM_ID is not a team id"; exit 1; }
fi

out_dir=${MAC_OUT:-$RUNNER_TEMP/mac-direct}
keys=$RUNNER_TEMP/mac-direct-keys
keychain=$RUNNER_TEMP/mac-direct.keychain-db
search_list=()
restore_search_list=no
cleanup() {
  if [ "$restore_search_list" = yes ]; then security list-keychains -d user -s "${search_list[@]}" || true; fi
  [ ! -f "$keychain" ] || security delete-keychain "$keychain" || true
  rm -rf "$keys" "$RUNNER_TEMP/ExportOptionsMacDirect.plist"
  [ -z "${mounted:-}" ] || hdiutil detach "$mounted" -quiet || true
}
trap cleanup EXIT
plist() { /usr/libexec/PlistBuddy -c "Print :$1" "$2/Contents/Info.plist"; }

project_id=$(awk '/^  TrommiMac:/{t=1} t && /PRODUCT_BUNDLE_IDENTIFIER:/{print $2; exit}' "$appstore/project.yml")
version=$(sed -n 's/^ *MARKETING_VERSION: *"\{0,1\}\([^"]*\)"\{0,1\} *$/\1/p' "$appstore/project.yml")
[ -n "$project_id" ] && [ -n "$version" ] || { echo "::error::project.yml names no TrommiMac bundle id or MARKETING_VERSION"; exit 1; }
[ -f "$repo/core/swift/TrommiCoreRust/lib/macos/libtrommi_core_ffi.a" ] || { echo "::error::no Rust core for the Mac at core/swift/TrommiCoreRust/lib/macos"; exit 1; }
echo "building $project_id $version ($VERSION) for direct download from $(printf '%s' "$COMMIT" | cut -c1-7) ($mode)"

if [ "$mode" = build ]; then
  echo "== 1. The App Store Connect key and the Developer ID certificate =="
  umask 077
  rm -rf "$keys"
  mkdir -p "$keys"
  key=$keys/AuthKey.p8
  body=$(printf '%s' "$APPLE_ASC_KEY" | sed -e 's/-----[A-Z ]*-----//g' | tr -d ' \n\r\t')
  printf -- '-----BEGIN PRIVATE KEY-----\n%s\n-----END PRIVATE KEY-----\n' "$(printf '%s' "$body" | fold -w 64)" > "$key"
  openssl pkey -in "$key" -noout 2>/dev/null || { echo "::error::APPLE_ASC_KEY is not readable as a private key"; exit 1; }
  python3 -c 'import base64, os, sys; sys.stdout.buffer.write(base64.b64decode(os.environ["APPLE_DEVID_P12"], validate=False))' > "$keys/devid.p12" \
    || { echo "::error::APPLE_DEVID_P12 is not base64"; exit 1; }
  # Apple's intermediate for Developer ID certificates made since 2019 (G2), by its SHA-256: codesign needs the chain
  curl -fsSL --retry 3 -o "$keys/DeveloperIDG2CA.cer" https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer
  echo "f16cd3c54c7f83cea4bf1a3e6a0819c8aaa8e4a1528fd144715f350643d2df3a  $keys/DeveloperIDG2CA.cer" | shasum -a 256 -c - > /dev/null \
    || { echo "::error::Apple's Developer ID G2 intermediate is not the expected file"; exit 1; }
  umask 022
  keychain_password=$(openssl rand -hex 32)
  rm -f "$keychain"
  security create-keychain -p "$keychain_password" "$keychain"
  security set-keychain-settings -lut 3600 "$keychain"
  security unlock-keychain -p "$keychain_password" "$keychain"
  security import "$keys/DeveloperIDG2CA.cer" -k "$keychain" > /dev/null
  security import "$keys/devid.p12" -k "$keychain" -f pkcs12 -P "$APPLE_DEVID_P12_PASSWORD" \
    -T /usr/bin/codesign -T /usr/bin/security > /dev/null || { echo "::error::the .p12 could not be imported (APPLE_DEVID_P12_PASSWORD?)"; exit 1; }
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" > /dev/null
  rm -f "$keys/devid.p12"
  # Xcode's export finds identities only through the search list: this keychain first, the others after it
  while IFS= read -r line; do
    line=$(printf '%s' "$line" | sed -e 's/^ *"//' -e 's/" *$//')
    [ -z "$line" ] || search_list+=("$line")
  done < <(security list-keychains -d user)
  restore_search_list=yes
  security list-keychains -d user -s "$keychain" "${search_list[@]}"
  identities=$(security find-identity -v -p codesigning "$keychain" | grep -F "\"Developer ID Application: " | grep -F "($APPLE_TEAM_ID)\"" || true)
  [ "$(printf '%s' "$identities" | grep -c .)" = 1 ] \
    || { echo "::error::the .p12 holds not exactly one valid Developer ID Application identity of team $APPLE_TEAM_ID"; exit 1; }
  identity=$(printf '%s' "$identities" | awk '{print $2}')
  printf '%s' "$identity" | grep -Eq '^[0-9A-F]{40}$' || { echo "::error::no fingerprint for the Developer ID identity"; exit 1; }
  echo "Developer ID Application of team $APPLE_TEAM_ID, certificate SHA-1 $identity"
  auth=(-allowProvisioningUpdates -authenticationKeyPath "$key" -authenticationKeyID "$APPLE_ASC_KEY_ID" -authenticationKeyIssuerID "$APPLE_ASC_ISSUER_ID")
fi

echo "== 2. Project (XcodeGen) =="
xcodebuild -version
"$XCODEGEN" generate --spec "$appstore/project.yml"

echo "== 3. Archive =="
# As mac_testflight.sh: built with the development entitlements (Xcode refuses an ad hoc Mac build whose entitlements
# need a profile), then signed ad hoc again with the store's TrommiMac.entitlements; the export takes them from there.
archive=$RUNNER_TEMP/TrommiMacDirect.xcarchive
rm -rf "$archive"
xcodebuild archive \
  -project "$appstore/Trommi.xcodeproj" -scheme TrommiMac -configuration Release \
  -destination 'generic/platform=macOS' -archivePath "$archive" \
  -derivedDataPath "$RUNNER_TEMP/DerivedDataMacDirect" \
  CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY=- AD_HOC_CODE_SIGNING_ALLOWED=YES \
  PROVISIONING_PROFILE_SPECIFIER= PROVISIONING_PROFILE= \
  TROMMI_MAC_ENTITLEMENTS=../Mac/TrommiMac.dev.entitlements \
  ${APPLE_TEAM_ID:+DEVELOPMENT_TEAM="$APPLE_TEAM_ID"} CURRENT_PROJECT_VERSION="$VERSION"
app=$archive/Products/Applications/Trommi.app
codesign --force --sign - --options runtime --entitlements "$appstore/TrommiMac.entitlements" --generate-entitlement-der "$app"
codesign --verify --deep --strict "$app"
python3 "$here/ios_entitlements.py" "$appstore" "$app" --mac
[ "$(plist CFBundleIdentifier "$app")" = "$project_id" ] && [ "$(plist CFBundleShortVersionString "$app")" = "$version" ] \
  && [ "$(plist CFBundleVersion "$app")" = "$VERSION" ] || { echo "::error::the archive is not $project_id $version ($VERSION)"; exit 1; }
lipo -archs "$app/Contents/MacOS/Trommi"

if [ "$mode" = build ]; then
  echo "== 4. Export (Developer ID) =="
  cat > "$RUNNER_TEMP/ExportOptionsMacDirect.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>developer-id</string>
  <key>destination</key><string>export</string>
  <key>teamID</key><string>$APPLE_TEAM_ID</string>
  <key>signingStyle</key><string>automatic</string>
  <key>signingCertificate</key><string>Developer ID Application</string>
</dict>
</plist>
EOF
  rm -rf "$RUNNER_TEMP/signed-mac-direct"
  xcodebuild -exportArchive \
    -archivePath "$archive" -exportOptionsPlist "$RUNNER_TEMP/ExportOptionsMacDirect.plist" \
    -exportPath "$RUNNER_TEMP/signed-mac-direct" \
    "${auth[@]}"
  app=$RUNNER_TEMP/signed-mac-direct/Trommi.app
  [ -d "$app" ] || { echo "::error::the export made no Trommi.app"; exit 1; }

  echo "== 5. The signature =="
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign -dvv "$app" > "$RUNNER_TEMP/codesign.txt" 2>&1
  grep -E '^(Authority|TeamIdentifier|Timestamp|CodeDirectory)' "$RUNNER_TEMP/codesign.txt"
  sed -n 's/^Authority=//p' "$RUNNER_TEMP/codesign.txt" | head -n 1 | grep -Eq "^Developer ID Application: .* \($APPLE_TEAM_ID\)$" \
    || { echo "::error::the app is not signed by a Developer ID Application certificate of team $APPLE_TEAM_ID"; exit 1; }
  grep -Eq '^CodeDirectory .*flags=0x[0-9a-f]*\(.*runtime' "$RUNNER_TEMP/codesign.txt" || { echo "::error::the app has no hardened runtime"; exit 1; }
  grep -q '^Timestamp=' "$RUNNER_TEMP/codesign.txt" || { echo "::error::the signature has no secure timestamp"; exit 1; }
  # the certificate that signed is the one of the .p12, not one Xcode chose or made by itself
  rm -f "$RUNNER_TEMP"/mac-direct-cert*
  codesign -d --extract-certificates="$RUNNER_TEMP/mac-direct-cert" "$app" 2> /dev/null
  leaf=$(shasum -a 1 "$RUNNER_TEMP/mac-direct-cert0" | awk '{print toupper($1)}')
  [ "$leaf" = "$identity" ] || { echo "::error::the app is signed with certificate $leaf, not the .p12's $identity"; exit 1; }
  profile=$app/Contents/embedded.provisionprofile
  [ -f "$profile" ] || { echo "::error::the app carries no provisioning profile (push and associated domains need one)"; exit 1; }
  security cms -D -i "$profile" > "$RUNNER_TEMP/profile.plist"
  [ "$(/usr/libexec/PlistBuddy -c 'Print :ProvisionsAllDevices' "$RUNNER_TEMP/profile.plist")" = true ] \
    || { echo "::error::the profile is not a Developer ID profile"; exit 1; }
  python3 "$here/ios_entitlements.py" "$appstore" "$app" --mac --distribution
  [ "$(plist CFBundleIdentifier "$app")" = "$project_id" ] && [ "$(plist CFBundleVersion "$app")" = "$VERSION" ] \
    || { echo "::error::the exported app is not $project_id ($VERSION)"; exit 1; }

  echo "== 6. Notarize the app =="
  notarize() {   # notarize <file>: submitted, waited for, Accepted, or the log and the end
    result=$RUNNER_TEMP/notary.json
    xcrun notarytool submit "$1" --key "$key" --key-id "$APPLE_ASC_KEY_ID" --issuer "$APPLE_ASC_ISSUER_ID" \
      --wait --timeout 60m --output-format json > "$result" || true
    status=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1])).get("status", ""))' "$result" 2>/dev/null || true)
    id=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1])).get("id", ""))' "$result" 2>/dev/null || true)
    echo "notarization of $(basename "$1"): ${status:-no answer} (${id:-no id})"
    if [ "$status" != Accepted ]; then
      if printf '%s' "$id" | grep -Eq '^[0-9a-f-]{36}$'; then
        xcrun notarytool log "$id" --key "$key" --key-id "$APPLE_ASC_KEY_ID" --issuer "$APPLE_ASC_ISSUER_ID" || true
      fi
      echo "::error::Apple did not notarize $(basename "$1") (${status:-no answer})"
      exit 1
    fi
  }
  ditto -c -k --keepParent "$app" "$RUNNER_TEMP/Trommi-notarize.zip"
  notarize "$RUNNER_TEMP/Trommi-notarize.zip"
  rm -f "$RUNNER_TEMP/Trommi-notarize.zip"
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  spctl --assess --type execute -vv "$app" 2>&1 | tee "$RUNNER_TEMP/spctl.txt"
  grep -q 'source=Notarized Developer ID' "$RUNNER_TEMP/spctl.txt" || { echo "::error::Gatekeeper does not accept the app as notarized Developer ID"; exit 1; }
fi

echo "== 7. Disk image =="
rm -rf "$out_dir" "$RUNNER_TEMP/dmg-stage"
mkdir -p "$out_dir" "$RUNNER_TEMP/dmg-stage"
ditto "$app" "$RUNNER_TEMP/dmg-stage/Trommi.app"
ln -s /Applications "$RUNNER_TEMP/dmg-stage/Applications"
dmg=$out_dir/$name.dmg
hdiutil create -volname Trommi -srcfolder "$RUNNER_TEMP/dmg-stage" -fs HFS+ -format UDZO -ov "$dmg" > /dev/null
rm -rf "$RUNNER_TEMP/dmg-stage"
if [ "$mode" = build ]; then
  codesign --sign "$identity" --keychain "$keychain" --timestamp "$dmg"
  codesign --verify --strict --verbose=2 "$dmg"
  notarize "$dmg"
  xcrun stapler staple "$dmg"
  xcrun stapler validate "$dmg"
  spctl --assess --type open --context context:primary-signature -vv "$dmg" 2>&1 | tee "$RUNNER_TEMP/spctl.txt"
  grep -q 'source=Notarized Developer ID' "$RUNNER_TEMP/spctl.txt" || { echo "::error::Gatekeeper does not accept the disk image as notarized Developer ID"; exit 1; }
else
  codesign --sign - "$dmg"
  codesign --verify --strict --verbose=2 "$dmg"
fi
hdiutil verify "$dmg" > /dev/null

echo "== 8. What is in the disk image =="
mounted=$(mktemp -d "$RUNNER_TEMP/mac-direct-mount.XXXXXX")
hdiutil attach "$dmg" -nobrowse -readonly -noautoopen -mountpoint "$mounted" > /dev/null
inside=$mounted/Trommi.app
codesign --verify --deep --strict "$inside"
[ "$(plist CFBundleIdentifier "$inside")" = "$project_id" ] && [ "$(plist CFBundleVersion "$inside")" = "$VERSION" ] \
  || { echo "::error::the disk image does not hold $project_id ($VERSION)"; exit 1; }
if [ "$mode" = build ]; then
  xcrun stapler validate "$inside"
  spctl --assess --type execute -vv "$inside" 2>&1 | grep -q 'source=Notarized Developer ID' \
    || { echo "::error::Gatekeeper does not accept the app in the disk image"; exit 1; }
else
  # ad hoc: Gatekeeper must refuse it; were it accepted, this check means nothing
  if spctl --assess --type execute -vv "$inside" > "$RUNNER_TEMP/spctl.txt" 2>&1; then
    cat "$RUNNER_TEMP/spctl.txt"; echo "::error::Gatekeeper accepts an ad hoc signed app"; exit 1
  fi
  echo "Gatekeeper refuses the ad hoc app, as it must: $(tr '\n' ' ' < "$RUNNER_TEMP/spctl.txt")"
fi
hdiutil detach "$mounted" -quiet
mounted=

(cd "$out_dir" && shasum -a 256 "$name.dmg" > "$name.dmg.sha256" && shasum -a 256 -c "$name.dmg.sha256")
ls -l "$out_dir"
out version "$version"
out build "$VERSION"
if [ "$mode" = build ]; then
  out ready yes
  summary "### Mac (direct download): $name.dmg, $project_id $version ($VERSION), Developer ID, notarized and stapled"
fi
