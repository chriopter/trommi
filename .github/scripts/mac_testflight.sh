#!/bin/bash
# The TestFlight delivery of the Mac app (the target TrommiMac of AppStore/project.yml; app/ios/README.md "On a Mac")
# on a macOS runner; ship-apple.yml runs it under `op run` beside the iOS delivery, with the same key. Steps: the bundle
# id and whether its app record has the macOS platform (asc.py prepare-mac; while it has not, nothing is built and the
# job ends green saying so) -> the build number (the highest Mac build App Store Connect has, plus one) ->
# Trommi.xcodeproj (XcodeGen) -> archive, signed ad hoc as the iOS one (nothing is created on the Apple account; the
# entitlements of TrommiMac.entitlements are checked) -> export for App Store Connect: Xcode signs with the team's
# cloud-managed "Apple Distribution" certificate, makes the Mac App Store profile, and signs the .pkg with the
# cloud-managed "Mac Installer Distribution" certificate, all through the Admin key -> the app inside that .pkg is
# checked (bundle id, version, build, entitlements, distribution) -> (main: the release of this run is published and
# verified) -> upload of that .pkg (asc.py upload, Build Uploads API, platform MAC_OS). `after`: processed, the group
# "Intern" holds it, What to Test (the external group and Beta App Review stay with the iOS app for now).
#   mac_testflight.sh upload       up to the upload; writes version, build and ready to GITHUB_OUTPUT
#   mac_testflight.sh after        processed, group, What to Test for TF_VERSION and TF_BUILD (Linux is enough)
#
# Environment as ios_testflight.sh (APPLE_ASC_KEY, APPLE_ASC_KEY_ID, APPLE_ASC_ISSUER_ID, APPLE_TEAM_ID from the
# 1Password Environment; RUNNER_TEMP, COMMIT, XCODEGEN; TF_GATE, TF_DRY_RUN, TF_UPLOADER, TESTFLIGHT_TESTER, NOTES),
# and MAC_BUNDLE_ID (default: the PRODUCT_BUNDLE_IDENTIFIER of TrommiMac in project.yml). TF_UPLOADER=xcode: a
# second Xcode export with destination upload instead of the API. The Rust core for the Mac must be in place
# (core/bindings/swift/TrommiCoreRust/lib/macos, from the job ios's artifact). Nothing here prints a value of the key.
set -euo pipefail

mode=${1:-upload}
case "$mode" in
  upload) required="APPLE_ASC_KEY APPLE_ASC_KEY_ID APPLE_ASC_ISSUER_ID APPLE_TEAM_ID RUNNER_TEMP COMMIT XCODEGEN" ;;
  after) required="APPLE_ASC_KEY APPLE_ASC_KEY_ID APPLE_ASC_ISSUER_ID RUNNER_TEMP COMMIT TF_VERSION TF_BUILD" ;;
  *) echo "usage: mac_testflight.sh upload|after" >&2; exit 2 ;;
esac
[ "${TF_GATE:-}" != release ] || required="$required GH_TOKEN GITHUB_REPOSITORY RUN_ID VERSION_RELEASE"
case "${TF_GATE:-}" in ''|release) ;; *) echo "::error::TF_GATE is release or empty, not ${TF_GATE}"; exit 1 ;; esac
for name in $required; do
  [ -n "${!name:-}" ] || { echo "::error::$name is not set for this run"; exit 1; }
done

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
appstore=$repo/app/ios/TrommiApp/AppStore
# the bundle id of the Mac target as project.yml names it (the one the archive gets)
project_id=$(awk '/^  TrommiMac:/{t=1} t && /PRODUCT_BUNDLE_IDENTIFIER:/{print $2; exit}' "$appstore/project.yml")
BUNDLE_ID=${MAC_BUNDLE_ID:-$project_id}
[ "$BUNDLE_ID" = "$project_id" ] || { echo "::error::MAC_BUNDLE_ID $BUNDLE_ID is not the TrommiMac bundle id of project.yml ($project_id)"; exit 1; }
export BUNDLE_ID ASC_PLATFORM=MAC_OS

pem() {
  body=$(printf '%s' "$1" | sed -e 's/-----[A-Z ]*-----//g' | tr -d ' \n\r\t')
  printf -- '-----BEGIN PRIVATE KEY-----\n%s\n-----END PRIVATE KEY-----\n' "$(printf '%s' "$body" | fold -w 64)"
}
keys=$RUNNER_TEMP/asc-mac
cleanup() { rc=$?; rm -rf "$keys"; exit "$rc"; }  # the status of the run, not of the cleanup (bash 3.2 on macOS lets the trap's last command win)
trap cleanup EXIT
umask 077
mkdir -p "$keys"
key=$keys/AuthKey.p8
pem "$APPLE_ASC_KEY" > "$key"
chmod 600 "$key"
openssl pkey -in "$key" -noout 2>/dev/null || { echo "::error::APPLE_ASC_KEY is not readable as a private key"; exit 1; }
umask 022
export ASC_KEY_ID=$APPLE_ASC_KEY_ID ASC_ISSUER_ID=$APPLE_ASC_ISSUER_ID ASC_KEY_PATH=$key
asc() { python3 "$appstore/asc.py" "$@"; }
auth=(-allowProvisioningUpdates -authenticationKeyPath "$key" -authenticationKeyID "$APPLE_ASC_KEY_ID" -authenticationKeyIssuerID "$APPLE_ASC_ISSUER_ID")
out() { [ -z "${GITHUB_OUTPUT:-}" ] || echo "$1=$2" >> "$GITHUB_OUTPUT"; }
summary() { [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "$1" >> "$GITHUB_STEP_SUMMARY"; }

if [ "$mode" = after ]; then
  printf '%s' "$TF_BUILD" | grep -Eq '^[1-9][0-9]*$' || { echo "::error::not a build number: $TF_BUILD"; exit 1; }
  printf '%s' "$TF_VERSION" | grep -Eq '^[0-9]+(\.[0-9]+)*$' || { echo "::error::not a version: $TF_VERSION"; exit 1; }
  echo "== 7. App Store Connect processes the Mac build =="
  asc wait "$TF_VERSION" "$TF_BUILD" | tee "$RUNNER_TEMP/wait-mac.txt"
  build_id=$(sed -n 's/^build_id: //p' "$RUNNER_TEMP/wait-mac.txt" | tail -n 1)
  [ -n "$build_id" ] || { echo "::error::App Store Connect named no build"; exit 1; }
  echo "== 8. Internal group \"Intern\" =="
  asc internal "$build_id" Intern "${TESTFLIGHT_TESTER:-}"
  echo "== 9. What to Test =="
  notes=${NOTES:-}
  if [ -z "$notes" ]; then
    # the newest day of What's new, as the web app serves it
    changes=$(curl -fsS --max-time 20 https://app.trommi.com/whats-new.json -o "$RUNNER_TEMP/whats-new.json" && asc whats-new "$RUNNER_TEMP/whats-new.json") \
      || changes="What changed: What's new in the app's menu."
    notes="Mac build $TF_BUILD ($(printf '%s' "$COMMIT" | cut -c1-7)). $changes"
  fi
  asc notes "$build_id" "$notes" en-US
  summary "### Mac: TestFlight build $TF_BUILD"
  summary ""
  summary "Version $TF_VERSION, build $TF_BUILD, commit \`$COMMIT\`, in the group Intern."
  exit 0
fi

echo "== 1. Bundle id and app record (Mac) =="
asc prepare-mac "$BUNDLE_ID" | tee "$RUNNER_TEMP/prepare-mac.txt"
ready=$(sed -n 's/^ready: //p' "$RUNNER_TEMP/prepare-mac.txt" | tail -n 1)
out ready "$ready"
if [ "$ready" != yes ]; then
  summary "### Mac: not delivered: the app record of $BUNDLE_ID has no macOS platform yet (app/ios/README.md \"On a Mac\")"
  exit 0
fi

echo "== 2. Version and build number =="
version=$(sed -n 's/^ *MARKETING_VERSION: *"\{0,1\}\([^"]*\)"\{0,1\} *$/\1/p' "$appstore/project.yml")
[ -n "$version" ] || { echo "::error::no MARKETING_VERSION in project.yml"; exit 1; }
build=$(asc next-build)
printf '%s' "$build" | grep -Eq '^[1-9][0-9]*$' || { echo "::error::no build number from App Store Connect"; exit 1; }
short=$(printf '%s' "$COMMIT" | cut -c1-7)
echo "building $BUNDLE_ID $version ($build) for macOS from $short"
lib=$repo/core/bindings/swift/TrommiCoreRust/lib/macos/libtrommi_core_ffi.a
[ -f "$lib" ] || { echo "::error::no Rust core for the Mac at core/bindings/swift/TrommiCoreRust/lib/macos"; exit 1; }

echo "== 3. Project (XcodeGen) =="
xcodebuild -version
"$XCODEGEN" generate --spec "$appstore/project.yml"

echo "== 4. Archive =="
# Signed ad hoc, as the iOS archive (ios_archive.sh says why): no profile, no key, nothing made on the account. The
# export below signs for the store and takes the entitlements from this signature. On macOS, Xcode refuses an ad hoc
# build whose entitlements need a profile (push, associated domains), so the archive is built with the development
# file (sandbox, network, files) and the app is then signed ad hoc once more with the store's TrommiMac.entitlements.
archive=$RUNNER_TEMP/TrommiMac.xcarchive
xcodebuild archive \
  -project "$appstore/Trommi.xcodeproj" -scheme TrommiMac -configuration Release \
  -destination 'generic/platform=macOS' -archivePath "$archive" \
  -derivedDataPath "$RUNNER_TEMP/DerivedDataMac" \
  CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY=- AD_HOC_CODE_SIGNING_ALLOWED=YES \
  PROVISIONING_PROFILE_SPECIFIER= PROVISIONING_PROFILE= \
  TROMMI_MAC_ENTITLEMENTS=../Mac/TrommiMac.dev.entitlements \
  DEVELOPMENT_TEAM="$APPLE_TEAM_ID" CURRENT_PROJECT_VERSION="$build"
app=$archive/Products/Applications/Trommi.app
codesign --force --sign - --options runtime --entitlements "$appstore/TrommiMac.entitlements" --generate-entitlement-der "$app"
codesign --verify --deep --strict "$app"
python3 "$here/ios_entitlements.py" "$appstore" "$app" --mac

echo "== 5. What went into the archive =="
plist() { /usr/libexec/PlistBuddy -c "Print :$1" "$2/Contents/Info.plist"; }
echo "$(plist CFBundleIdentifier "$app") $(plist CFBundleShortVersionString "$app") ($(plist CFBundleVersion "$app"))"
[ "$(plist CFBundleIdentifier "$app")" = "$BUNDLE_ID" ] && [ "$(plist CFBundleVersion "$app")" = "$build" ] \
  || { echo "::error::the archive is not $BUNDLE_ID build $build"; exit 1; }
[ "$(plist ITSAppUsesNonExemptEncryption "$app")" = false ] || { echo "::error::the Mac app's ITSAppUsesNonExemptEncryption is not false"; exit 1; }
lipo -archs "$app/Contents/MacOS/Trommi"

echo "== 6. Export =="
export_options() {
  cat > "$RUNNER_TEMP/ExportOptionsMac.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>$1</string>
  <key>teamID</key><string>$APPLE_TEAM_ID</string>
  <key>signingStyle</key><string>automatic</string>
  <key>manageAppVersionAndBuildNumber</key><false/>
  <key>testFlightInternalTestingOnly</key><false/>
  <key>uploadSymbols</key><true/>
</dict>
</plist>
EOF
}
export_options export
xcodebuild -exportArchive \
  -archivePath "$archive" -exportOptionsPlist "$RUNNER_TEMP/ExportOptionsMac.plist" \
  -exportPath "$RUNNER_TEMP/signed-mac" \
  "${auth[@]}"
pkg=$(ls "$RUNNER_TEMP"/signed-mac/*.pkg)
pkgutil --check-signature "$pkg" | sed -n '1,4p'
# the app inside the very file that is uploaded
rm -rf "$RUNNER_TEMP/pkg-mac"
pkgutil --expand-full "$pkg" "$RUNNER_TEMP/pkg-mac"
signed=$(find "$RUNNER_TEMP/pkg-mac" -maxdepth 4 -type d -name Trommi.app | head -n 1)
[ -n "$signed" ] || { echo "::error::no Trommi.app in $pkg"; exit 1; }
[ "$(plist CFBundleIdentifier "$signed")" = "$BUNDLE_ID" ] && [ "$(plist CFBundleShortVersionString "$signed")" = "$version" ] \
  && [ "$(plist CFBundleVersion "$signed")" = "$build" ] || { echo "::error::the .pkg does not hold $BUNDLE_ID $version ($build)"; exit 1; }
[ -f "$signed/Contents/embedded.provisionprofile" ] || { echo "::error::the signed app carries no provisioning profile"; exit 1; }
python3 "$here/ios_entitlements.py" "$appstore" "$signed" --mac --distribution
codesign -dvv "$signed" 2>&1 | grep '^Authority=' | head -n 1 || true

if [ "${TF_GATE:-}" = release ]; then
  echo "== 6b. The release of this run =="
  gate='wait'
  [ "${TF_DRY_RUN:-}" = 1 ] && gate=verify
  VERSION=$VERSION_RELEASE bash "$here/ios_release_gate.sh" "$gate"
fi

echo "== 6c. Upload =="
if [ "${TF_DRY_RUN:-}" = 1 ]; then
  asc upload "$pkg" "$version" "$build" --dry-run
  echo "dry run: Mac build $version ($build) was NOT uploaded"
  summary "### Mac: dry run, build $version ($build) signed and checked, not uploaded"
  exit 0
fi
if [ "${TF_UPLOADER:-api}" = xcode ]; then
  export_options upload
  xcodebuild -exportArchive \
    -archivePath "$archive" -exportOptionsPlist "$RUNNER_TEMP/ExportOptionsMac.plist" \
    -exportPath "$RUNNER_TEMP/export-mac" \
    "${auth[@]}"
else
  asc upload "$pkg" "$version" "$build"
fi
rm -f "$RUNNER_TEMP/ExportOptionsMac.plist"
out version "$version"
out build "$build"
echo "uploaded: $BUNDLE_ID $version ($build) for macOS from $short"
