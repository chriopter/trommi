#!/usr/bin/env python3
"""App Store Connect API steps of the TestFlight workflow (.github/workflows/deploy_ios.yml). Standard library only; the
JWT is signed with the openssl command line tool, so it runs on a stock macOS runner and on Linux alike.

  asc.py prepare BUNDLE_ID          the bundle id (Push Notifications, App Groups, Associated Domains) and
                                    BUNDLE_ID.share, .notify, .live (App Groups) are registered, their INVALID App Store
                                    profiles removed; the app record exists
  asc.py wait VERSION BUILD         waits until App Store Connect has processed that build (VALID), then prints its id
  asc.py internal BUILD_ID GROUP [EMAIL]
                                    the internal TestFlight group GROUP exists (every build), holds BUILD_ID, and
                                    EMAIL (a user of the team) is one of its testers
  asc.py notes BUILD_ID TEXT [LOCALE]
                                    TestFlight "What to Test" of that build (every localization; LOCALE, default
                                    de-DE, if none)
  asc.py external BUILD_ID GROUP    the external TestFlight group GROUP (made once by hand in App Store Connect; fails
                                    saying so if it is missing) holds BUILD_ID; prints its public link, if enabled
  asc.py beta-review BUILD_ID       the beta app's test information (Beta App Review contact, demo account, notes from
                                    metadata/review_information, description and feedback address) is filled in, and
                                    the build goes to Beta App Review when Apple asks for it (READY_FOR_BETA_SUBMISSION
                                    and no submission yet; later builds of a reviewed version usually need none).
                                    The demo account's password comes from APPSTORE_DEMO_PASSWORD
  asc.py whats-new CHANGELOG        offline: the newest day of CHANGELOG.md as plain text for "What to Test"
  asc.py upload IPA VERSION BUILD [--dry-run]
                                    uploads the signed .ipa through the Build Uploads API (no second Xcode export),
                                    after checking it is BUNDLE_ID VERSION (BUILD); waits until the upload is COMPLETE.
                                    --dry-run: the same checks, offline, and prints what would be sent; nothing is sent
  asc.py next-build                 the highest build number App Store Connect has for the app, plus one
  asc.py last-sha                   the commit of the newest build whose What to Test names one ("(abc1234)")
  asc.py crashes OUT_DIR [SINCE]    read only: TestFlight crash feedback newer than SINCE (ISO 8601, default 3 days
                                    ago) with each crash log as a file, the diagnostic signatures (hangs, launches,
                                    disk writes) of the three newest builds, summary.md (tester comments included) and
                                    issue.md (without them); no tester e-mail or name is asked for or written
  asc.py export-code                the code of the app's APPROVED export compliance documentation (the newest if
                                    several), read only; fails listing every declaration's state and date if none is

Environment: ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH (the .p8 file). Nothing here prints key material or tokens.
"""
import base64, datetime, hashlib, json, os, plistlib, re, subprocess, sys, time, urllib.error, urllib.parse, urllib.request, zipfile

API = "https://api.appstoreconnect.apple.com"


def die(msg):
    # (on stderr: the script takes some commands' stdout as their value, and the message must still be seen)
    print(f"::error::{msg}", file=sys.stderr, flush=True)
    sys.exit(1)


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def der_to_raw(sig):
    # ECDSA-Sig-Value ::= SEQUENCE { r INTEGER, s INTEGER } -> r || s, 32 bytes each (JWS ES256).
    assert sig[0] == 0x30
    i = 2 if sig[1] < 0x80 else 2 + (sig[1] & 0x7F)
    out = b""
    for _ in range(2):
        assert sig[i] == 0x02
        n = sig[i + 1]
        out += sig[i + 2:i + 2 + n].lstrip(b"\0").rjust(32, b"\0")
        i += 2 + n
    return out


_token = (0, "")


def token():
    global _token
    now = int(time.time())
    if _token[0] > now + 60:
        return _token[1]
    key_id, issuer, path = os.environ["ASC_KEY_ID"], os.environ["ASC_ISSUER_ID"], os.environ["ASC_KEY_PATH"]
    head = b64url(json.dumps({"alg": "ES256", "kid": key_id, "typ": "JWT"}).encode())
    claims = b64url(json.dumps({"iss": issuer, "iat": now, "exp": now + 1200, "aud": "appstoreconnect-v1"}).encode())
    signed = f"{head}.{claims}".encode()
    der = subprocess.run(["openssl", "dgst", "-sha256", "-sign", path], input=signed, capture_output=True, check=True).stdout
    _token = (now + 1200, f"{head}.{claims}.{b64url(der_to_raw(der))}")
    return _token[1]


def call(method, path, body=None, ok=()):
    """ok: HTTP error codes that are expected answers (the call then returns {})."""
    req = urllib.request.Request(API + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"Authorization": f"Bearer {token()}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            raw = resp.read()
    except urllib.error.HTTPError as e:
        text = e.read().decode(errors="replace")
        if e.code in ok:
            errs = _errors(text)
            return {"error": e.code, "codes": [x.get("code", "") for x in errs],
                    "detail": "; ".join(x.get("detail", "") for x in errs)}
        die(f"{method} {path}: HTTP {e.code} {text}")
    return json.loads(raw) if raw else {}


def _errors(text):
    try:
        return json.loads(text).get("errors", [])
    except ValueError:
        return [{"detail": text[:300]}]


def rel(kind, ident):
    return {"data": {"type": kind, "id": ident}}


def q(s):
    return urllib.parse.quote(s, safe="")


def app_id(bundle):
    apps = [a for a in call("GET", f"/v1/apps?filter[bundleId]={q(bundle)}")["data"]
            if a["attributes"]["bundleId"] == bundle]
    return apps[0]["id"] if apps else None


def bundle_id(identifier, name, wanted):
    """The bundle id registered, with every capability in `wanted` turned on."""
    found = [b for b in call("GET", f"/v1/bundleIds?filter[identifier]={q(identifier)}&limit=200")["data"]
             if b["attributes"]["identifier"] == identifier]
    if found:
        bid = found[0]["id"]
    else:
        print(f"registering bundle id {identifier}")
        bid = call("POST", "/v1/bundleIds", {"data": {"type": "bundleIds", "attributes": {
            "identifier": identifier, "name": name, "platform": "IOS"}}})["data"]["id"]
    caps = [c["attributes"]["capabilityType"] for c in call("GET", f"/v1/bundleIds/{bid}/bundleIdCapabilities")["data"]]
    for cap in wanted:
        if cap not in caps:
            print(f"turning on {cap} for {identifier}")
            call("POST", "/v1/bundleIdCapabilities", {"data": {"type": "bundleIdCapabilities",
                 "attributes": {"capabilityType": cap}, "relationships": {"bundleId": rel("bundleIds", bid)}}})
    print(f"bundle id {identifier}: {bid}, capabilities {sorted(set(caps) | set(wanted))}")
    return bid


def prepare(bundle):
    # The app (push, the App Group of its extensions, universal links of app.trommi.com) and its extensions: Share, the
    # Notification Service Extension, the Live Activity widget (each the App Group). The API turns the App Groups
    # capability on but cannot create the group or assign it: group.com.trommi.ios is made and assigned to every id once
    # in the developer portal (app/ios/README.md "Share Extension"). Communication Notifications (the session's drawing on a
    # push) is no capability type of the API: ticked once in the portal (app/ios/README.md "Push").
    bundle_id(bundle, "Trommi", ["PUSH_NOTIFICATIONS", "APP_GROUPS", "ASSOCIATED_DOMAINS"])
    extensions = [(f"{bundle}.share", "Trommi Share"), (f"{bundle}.notify", "Trommi Notify"), (f"{bundle}.live", "Trommi Live")]
    for ident, name in extensions:
        bundle_id(ident, name, ["APP_GROUPS"])
    # A capability change makes the profiles INVALID; their names stay taken, and omarchy-apple-dev's identity step
    # (which looks only at ACTIVE ones) could not make new ones under the same name (409). They go here.
    for ident in (bundle, *(i for i, _ in extensions)):
        for p in call("GET", f"/v1/profiles?filter[profileType]=IOS_APP_STORE&limit=200")["data"]:
            name, state = p["attributes"]["name"], p["attributes"]["profileState"]
            if name.startswith(f"omarchy-apple-dev {ident} ") and state != "ACTIVE":
                print(f"removing the {state} profile '{name}'")
                call("DELETE", f"/v1/profiles/{p['id']}")
    aid = app_id(bundle)
    if not aid:
        die(f"no App Store Connect app record for {bundle}: create it once in App Store Connect (Apps > + > New App; "
            "the API cannot create apps), app/ios/README.md 'TestFlight from CI'")
    gh_out("app_id", aid)


def wait(version, build):
    aid = app_id(os.environ["BUNDLE_ID"]) or die("no app record")
    # The build shows up a few minutes after the upload, then goes PROCESSING -> VALID (or FAILED / INVALID).
    for i in range(80):
        rows = call("GET", f"/v1/builds?filter[app]={aid}&filter[version]={q(build)}"
                           f"&filter[preReleaseVersion.version]={q(version)}&limit=1")["data"]
        state = rows[0]["attributes"]["processingState"] if rows else "not there yet"
        print(f"attempt {i + 1}: build {version} ({build}): {state}", flush=True)
        if state == "VALID":
            attrs = rows[0]["attributes"]
            print(f"usesNonExemptEncryption: {attrs.get('usesNonExemptEncryption')}")
            if attrs.get("usesNonExemptEncryption") is None:
                print("::warning::export compliance is not answered for this build: testers cannot install it until "
                      "it is answered in App Store Connect (TestFlight > the build), or the variable "
                      "ITS_NON_EXEMPT_ENCRYPTION (YES or NO) answers it for every build (app/ios/README.md)")
            gh_out("build_id", rows[0]["id"])
            return
        if state in ("FAILED", "INVALID"):
            die(f"App Store Connect could not process build {version} ({build}): {state} (the e-mail to the account "
                "holder names the reason)")
        time.sleep(30)
    die(f"build {version} ({build}) was not processed within 40 minutes")


def internal(build_id, group, email=""):
    aid = app_id(os.environ["BUNDLE_ID"]) or die("no app record")
    groups = [g for g in call("GET", f"/v1/apps/{aid}/betaGroups?limit=200")["data"]
              if g["attributes"]["name"] == group]
    if groups:
        gid = groups[0]["id"]
        every = groups[0]["attributes"].get("hasAccessToAllBuilds")
    else:
        print(f"creating the internal group {group}")
        g = call("POST", "/v1/betaGroups", {"data": {"type": "betaGroups", "attributes": {
            "name": group, "isInternalGroup": True, "hasAccessToAllBuilds": True},
            "relationships": {"app": rel("apps", aid)}}})["data"]
        gid, every = g["id"], g["attributes"].get("hasAccessToAllBuilds")
    if not every:
        call("POST", f"/v1/betaGroups/{gid}/relationships/builds", {"data": [{"type": "builds", "id": build_id}]})
    print(f"group {group} ({gid}): build {build_id} {'(the group gets every build)' if every else 'added'}")
    if email:
        testers = call("GET", f"/v1/betaGroups/{gid}/betaTesters?limit=200")["data"]
        if any(t["attributes"].get("email", "").lower() == email.lower() for t in testers):
            print("the tester is in the group")
        else:
            # Internal testers must be users of the team; Apple answers 409 when the address is not one.
            added = call("POST", "/v1/betaTesters", {"data": {"type": "betaTesters", "attributes": {"email": email},
                         "relationships": {"betaGroups": {"data": [{"type": "betaGroups", "id": gid}]}}}}, ok=(409,))
            if "error" not in added:
                print("tester added to the group")
            else:
                users = call("GET", f"/v1/users?filter[username]={q(email)}&limit=5")["data"]
                roles = users[0]["attributes"].get("roles") if users else None
                print(f"team user {email}: {'roles ' + str(roles) if users else 'not found'}")
                print(f"::warning::App Store Connect did not take {email} as an internal tester (409: "
                      f"{added['detail']}): add the tester in App Store Connect (TestFlight > {group}); internal "
                      "testers must be users of the team")


def notes(build_id, text, locale="de-DE"):
    text = text.strip()[:4000]
    locs = call("GET", f"/v1/builds/{build_id}/betaBuildLocalizations?limit=50")["data"]
    for loc in locs:
        call("PATCH", f"/v1/betaBuildLocalizations/{loc['id']}", {"data": {"type": "betaBuildLocalizations",
             "id": loc["id"], "attributes": {"whatsNew": text}}})
        print(f"What to Test ({loc['attributes'].get('locale')}): {text}")
    if not locs:
        call("POST", "/v1/betaBuildLocalizations", {"data": {"type": "betaBuildLocalizations",
             "attributes": {"locale": locale, "whatsNew": text}, "relationships": {"build": rel("builds", build_id)}}})
        print(f"What to Test ({locale}): {text}")


def upload(ipa, version, build, *flags):
    dry = "--dry-run" in flags
    unknown = [f for f in flags if f != "--dry-run"]
    if unknown:
        die(f"upload: unknown option {unknown[0]}")
    bundle = os.environ["BUNDLE_ID"]
    with zipfile.ZipFile(ipa) as z:
        names = z.namelist()
        infos = [n for n in names if re.fullmatch(r"Payload/[^/]+\.app/Info\.plist", n)]
        if len(infos) != 1:
            die(f"{ipa}: not one app under Payload/ ({len(infos)})")
        info = plistlib.loads(z.read(infos[0]))
        symbols = sum(1 for n in names if n.startswith("Symbols/") and not n.endswith("/"))
    have = (info.get("CFBundleIdentifier"), info.get("CFBundleShortVersionString"), info.get("CFBundleVersion"))
    if have != (bundle, version, build):
        die(f"{ipa} is {have[0]} {have[1]} ({have[2]}), not {bundle} {version} ({build})")
    size = os.path.getsize(ipa)
    md5 = hashlib.md5()
    with open(ipa, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            md5.update(chunk)
    print(f"ipa: {bundle} {version} ({build}), {size} bytes, md5 {md5.hexdigest()}, {symbols} symbol file(s)")
    if symbols == 0:
        print("::warning::the .ipa carries no Symbols/: crash reports in TestFlight will not be symbolicated")
    if dry:
        print("dry run: would POST /v1/buildUploads (platform IOS), POST /v1/buildUploadFiles (com.apple.ipa), "
              "PUT the parts, PATCH uploaded=true with the MD5, then poll until COMPLETE; nothing was sent")
        return
    aid = app_id(bundle) or die("no app record")
    upload_id = call("POST", "/v1/buildUploads", {"data": {"type": "buildUploads", "attributes": {
        "cfBundleShortVersionString": version, "cfBundleVersion": build, "platform": "IOS"},
        "relationships": {"app": rel("apps", aid)}}})["data"]["id"]
    file = call("POST", "/v1/buildUploadFiles", {"data": {"type": "buildUploadFiles", "attributes": {
        "assetType": "ASSET", "fileName": os.path.basename(ipa), "fileSize": size, "uti": "com.apple.ipa"},
        "relationships": {"buildUpload": rel("buildUploads", upload_id)}}})["data"]
    ops = file["attributes"]["uploadOperations"]
    with open(ipa, "rb") as f:
        for op in ops:
            f.seek(op["offset"])
            part = f.read(op["length"])
            headers = {h["name"]: h["value"] for h in op.get("requestHeaders") or []}
            for attempt in range(3):
                try:
                    req = urllib.request.Request(op["url"], data=part, method=op["method"], headers=headers)
                    with urllib.request.urlopen(req, timeout=600) as resp:
                        resp.read()
                    break
                except (urllib.error.URLError, TimeoutError) as e:
                    if attempt == 2:
                        die(f"upload of the part at {op['offset']} failed: {e}")
                    time.sleep(5)
    print(f"uploaded {size} bytes in {len(ops)} part(s)", flush=True)
    call("PATCH", f"/v1/buildUploadFiles/{file['id']}", {"data": {"type": "buildUploadFiles", "id": file["id"],
         "attributes": {"uploaded": True, "sourceFileChecksums": {
             "file": {"hash": md5.hexdigest(), "algorithm": "MD5"}}}}})
    state = {}
    for _ in range(120):
        state = call("GET", f"/v1/buildUploads/{upload_id}")["data"]["attributes"].get("state") or {}
        if state.get("state") in ("COMPLETE", "FAILED"):
            break
        time.sleep(15)
    print(f"buildUpload {upload_id}: {state.get('state')}")
    for kind in ("errors", "warnings", "infos"):
        for item in state.get(kind) or []:
            print(f"  {kind[:-1]} {item.get('code')}: {item.get('description')}")
    if state.get("state") != "COMPLETE":
        die(f"App Store Connect did not take the upload ({state.get('state') or 'no answer'})")


def builds(aid):
    rows, path = [], f"/v1/builds?filter[app]={aid}&limit=200&fields[builds]=version,uploadedDate"
    while path:
        page = call("GET", path)
        rows += page["data"]
        nxt = page.get("links", {}).get("next")
        path = nxt[len(API):] if nxt else None
    return rows


def next_build():
    aid = app_id(os.environ["BUNDLE_ID"]) or die("no app record")
    numbers = [int(b["attributes"]["version"]) for b in builds(aid) if b["attributes"]["version"].isdigit()]
    print(max(numbers, default=0) + 1)


def last_sha():
    aid = app_id(os.environ["BUNDLE_ID"]) or die("no app record")
    newest = sorted(builds(aid), key=lambda b: b["attributes"].get("uploadedDate") or "", reverse=True)
    for b in newest[:20]:
        for loc in call("GET", f"/v1/builds/{b['id']}/betaBuildLocalizations?limit=50")["data"]:
            m = re.search(r"\(([0-9a-f]{7,40})\)", loc["attributes"].get("whatsNew") or "")
            if m:
                print(m.group(1))
                return


def pick_code(rows):
    """Of the app's encryption declarations (the API's `data`), the APPROVED one's code, the newest if several: (code,
    state, date). None when none is approved."""
    def date(r):
        a = r.get("attributes") or {}
        return a.get("createdDate") or a.get("uploadedDate") or ""
    approved = [r for r in rows if (r.get("attributes") or {}).get("appEncryptionDeclarationState") == "APPROVED"
                and (r.get("attributes") or {}).get("codeValue")]
    if not approved:
        return None
    best = sorted(approved, key=date, reverse=True)[0]
    return best["attributes"]["codeValue"], "APPROVED", date(best)


def export_code():
    aid = app_id(os.environ["BUNDLE_ID"])
    if not aid:
        print("::error::no app record", file=sys.stderr, flush=True)
        sys.exit(1)
    # (the top-level collection filtered by the app: /v1/apps/{id} has no relationship of that name)
    fields = "appEncryptionDeclarationState,codeValue,createdDate,uploadedDate"
    rows, path = [], f"/v1/appEncryptionDeclarations?filter[app]={q(aid)}&fields[appEncryptionDeclarations]={fields}&limit=200"
    while path:
        page = call("GET", path)
        rows += page["data"]
        nxt = page.get("links", {}).get("next")
        path = nxt[len(API):] if nxt else None
    picked = pick_code(rows)
    if not picked:
        listed = ", ".join(f"{(r.get('attributes') or {}).get('appEncryptionDeclarationState', '?')} "
                           f"({(r.get('attributes') or {}).get('createdDate') or (r.get('attributes') or {}).get('uploadedDate') or 'no date'})"
                           for r in rows) or "none"
        # (on stderr: stdout is the code the script takes)
        print(f"::error::no APPROVED export compliance documentation for {os.environ['BUNDLE_ID']} in App Store Connect "
              f"(declarations: {listed}); set the repository variable IOS_EXPORT_COMPLIANCE_CODE or wait for the approval",
              file=sys.stderr, flush=True)
        sys.exit(1)
    code, state, when = picked
    print(f"export compliance code from App Store Connect: {code} ({state}, {when or 'no date'})", file=sys.stderr)
    print(code)


# ---- the public beta: an external group, Beta App Review ------------------------------------------------------------

HERE = os.path.dirname(os.path.abspath(__file__))
META = os.path.join(HERE, "metadata")


def meta_text(*parts):
    path = os.path.join(META, *parts)
    return open(path, encoding="utf-8").read().strip() if os.path.exists(path) else ""


def external(build_id, group):
    aid = app_id(os.environ["BUNDLE_ID"]) or die("no app record")
    found = [g for g in call("GET", f"/v1/apps/{aid}/betaGroups?limit=200")["data"] if g["attributes"]["name"] == group]
    if not found:
        die(f"there is no TestFlight group \"{group}\" for {os.environ['BUNDLE_ID']}: create it once in App Store Connect "
            f"(the app > TestFlight > External Testing > +, name \"{group}\", then Enable Public Link) and run the "
            "delivery again (app/ios/README.md \"Public beta\")")
    g = found[0]
    if g["attributes"].get("isInternalGroup"):
        die(f"the TestFlight group \"{group}\" is an internal group; the public beta needs an external one of that name")
    call("POST", f"/v1/betaGroups/{g['id']}/relationships/builds", {"data": [{"type": "builds", "id": build_id}]})
    print(f"group {group} ({g['id']}, external): build {build_id} added")
    link = g["attributes"].get("publicLink") if g["attributes"].get("publicLinkEnabled") else None
    if link:
        gh_out("public_link", link)
    else:
        print(f"::warning::the group {group} has no public link enabled: App Store Connect > TestFlight > {group} > "
              "Public Link > Enable")


def review_detail():
    """The Beta App Review contact and demo account as metadata/review_information has them (empty values left out,
    so what was entered by hand in App Store Connect stays)."""
    demo = meta_text("review_information", "demo_user.txt")
    want = {"contactFirstName": meta_text("review_information", "first_name.txt"),
            "contactLastName": meta_text("review_information", "last_name.txt"),
            "contactEmail": meta_text("review_information", "email_address.txt"),
            "contactPhone": meta_text("review_information", "phone_number.txt"),
            "notes": meta_text("review_information", "notes.txt")[:4000],
            "demoAccountRequired": bool(demo)}
    if demo:
        want["demoAccountName"] = demo
        want["demoAccountPassword"] = os.environ.get("APPSTORE_DEMO_PASSWORD") or die(
            "APPSTORE_DEMO_PASSWORD is not set: Beta App Review signs in with the demo account "
            "(metadata/review_information/demo_user.txt); add its password to the 1Password Environment of `ios`")
    return {k: v for k, v in want.items() if v != ""}


def beta_review(build_id):
    aid = app_id(os.environ["BUNDLE_ID"]) or die("no app record")
    # 1. Test information of the app: contact and demo account for Beta App Review
    detail = call("GET", f"/v1/apps/{aid}/betaAppReviewDetail")["data"]
    want = review_detail()
    have = detail.get("attributes") or {}
    if not (want.get("contactPhone") or have.get("contactPhone")):
        die("Beta App Review needs a contact phone number: fill in metadata/review_information/phone_number.txt "
            "(or once in App Store Connect > TestFlight > Test Information)")
    call("PATCH", f"/v1/betaAppReviewDetails/{detail['id']}", {"data": {"type": "betaAppReviewDetails",
         "id": detail["id"], "attributes": want}})
    print(f"Beta App Review information set: {', '.join(k for k in sorted(want) if k != 'demoAccountPassword')}")
    # 2. Beta description and feedback address (every localization; en-US if none)
    loc_want = {"description": meta_text("en-US", "description.txt")[:4000],
                "feedbackEmail": meta_text("review_information", "email_address.txt")}
    locs = call("GET", f"/v1/apps/{aid}/betaAppLocalizations?limit=50")["data"]
    for loc in locs:
        missing = {k: v for k, v in loc_want.items() if v and not (loc.get("attributes") or {}).get(k)}
        if missing:
            call("PATCH", f"/v1/betaAppLocalizations/{loc['id']}", {"data": {"type": "betaAppLocalizations",
                 "id": loc["id"], "attributes": missing}})
            print(f"beta app localization {(loc.get('attributes') or {}).get('locale')}: set {', '.join(sorted(missing))}")
    if not locs:
        call("POST", "/v1/betaAppLocalizations", {"data": {"type": "betaAppLocalizations",
             "attributes": {"locale": "en-US", **{k: v for k, v in loc_want.items() if v}},
             "relationships": {"app": rel("apps", aid)}}})
        print("beta app localization en-US: created")
    # 3. Beta App Review of the build, when Apple asks for it
    state = (call("GET", f"/v1/builds/{build_id}/buildBetaDetail")["data"].get("attributes") or {}).get("externalBuildState")
    subs = call("GET", f"/v1/betaAppReviewSubmissions?filter[build]={q(build_id)}&limit=5")["data"]
    print(f"external state of build {build_id}: {state}; review submissions: {len(subs)}")
    if subs:
        print(f"already submitted ({(subs[0].get('attributes') or {}).get('betaReviewState')}): nothing to send")
    elif state == "READY_FOR_BETA_SUBMISSION":
        sent = call("POST", "/v1/betaAppReviewSubmissions", {"data": {"type": "betaAppReviewSubmissions",
                    "relationships": {"build": rel("builds", build_id)}}}, ok=(422,))
        if "ENTITY_UNPROCESSABLE.ANOTHER_BUILD_IN_REVIEW" in sent.get("codes", []):
            # Apple reviews one build of a train at a time: this one stays in its groups and is sent by the next
            # main build once that review is through (a build still READY_FOR_BETA_SUBMISSION then)
            print(f"build {build_number(build_id)} waits; build {builds_in_review(aid)} is in review")
            state = "WAITING_FOR_ANOTHER_REVIEW"
        elif "error" in sent:
            die(f"POST /v1/betaAppReviewSubmissions: HTTP {sent['error']} {', '.join(sent['codes'])}: {sent['detail']}")
        else:
            sub = sent["data"]
            print(f"submitted for Beta App Review: {sub['id']} ({(sub.get('attributes') or {}).get('betaReviewState')})")
    elif state == "MISSING_EXPORT_COMPLIANCE":
        die("export compliance of the build is not answered: Beta App Review cannot be asked (app/ios/README.md "
            "\"Export compliance\")")
    else:
        print(f"no Beta App Review needed now ({state})")
    gh_out("external_state", state)


def build_number(build_id):
    """The build's number (CFBundleVersion), for a log line; the id when App Store Connect does not say."""
    got = call("GET", f"/v1/builds/{build_id}?fields[builds]=version", ok=(400, 403, 404))
    return ((got.get("data") or {}).get("attributes") or {}).get("version") or build_id


def builds_in_review(aid):
    """The numbers of the app's builds waiting for or in Beta App Review, for a log line ("another" if unknown)."""
    got = call("GET", f"/v1/builds?filter[app]={q(aid)}&filter[betaAppReviewSubmission.betaReviewState]=WAITING_FOR_REVIEW,IN_REVIEW"
                      "&fields[builds]=version&limit=5", ok=(400, 403, 404))
    return ", ".join(str((b.get("attributes") or {}).get("version")) for b in got.get("data") or []) or "another"


def whats_new(changelog):
    """The newest day of CHANGELOG.md as plain text: "New:", "Better:", "Fixed:", one line per entry."""
    lines, out, inside = open(changelog, encoding="utf-8").read().splitlines(), [], False
    for line in lines:
        if line.startswith("## "):
            if inside:
                break
            inside = True
            continue
        if not inside or not line.strip():
            continue
        m = re.fullmatch(r"\*\*(.+?)\*\*\s*", line.strip())
        if m:
            out.append(f"{m.group(1)}:")
        elif line.startswith("- "):
            out.append("- " + line[2:].strip())
        elif out and out[-1].startswith("- "):
            out[-1] += " " + line.strip()
    if not out:
        die(f"{changelog}: no day with entries")
    text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", "\n".join(out)).replace("**", "").replace("`", "")
    # (room for the "Build N (commit)." in front, within What to Test's 4000 characters)
    if len(text) > 3900:
        text = text[:3890].rsplit("\n", 1)[0] + "\n…"
    print(text)


# ---- TestFlight crash reports (asc.py crashes; .github/workflows/ios_crashes.yml) ----
# Read only. The tester is never asked for (no `tester` field or include), and the submission's `email` is not
# requested; KEEP lists what is kept of a submission all the same, should the API send more. The tester's comment is
# kept for summary.md (the artifact) but never written to issue.md, which goes to a public issue.

CRASH_FIELDS = "createdDate,comment,deviceModel,osVersion,architecture,appPlatform,deviceFamily,buildBundleId,build,crashLog"
KEEP = ("createdDate", "comment", "deviceModel", "osVersion", "architecture", "appPlatform", "deviceFamily", "buildBundleId")
DIAGNOSTIC_TYPES = ("HANGS", "LAUNCHES", "DISK_WRITES")   # the API's diagnosticType values (no crashes among them)
FRAMES = 8


def when(text):
    """An ISO 8601 date or date-time (Z or an offset; a plain date is midnight UTC) as an aware datetime, or None."""
    if not text:
        return None
    try:
        d = datetime.datetime.fromisoformat(text.strip().replace("Z", "+00:00"))
    except ValueError:
        return None
    return d if d.tzinfo else d.replace(tzinfo=datetime.timezone.utc)


def pages(path, limit_pages=20):
    """Every row of a paged GET, following links.next; stops quietly on 400/403/404 (an API the key may not reach)."""
    rows, included = [], []
    for _ in range(limit_pages):
        page = call("GET", path, ok=(400, 403, 404))
        if "error" in page:
            print(f"::warning::GET {path.split('?')[0]}: HTTP {page['error']} {', '.join(page['codes'])}", file=sys.stderr)
            break
        rows += page.get("data") or []
        included += page.get("included") or []
        nxt = (page.get("links") or {}).get("next")
        if not nxt or not nxt.startswith(API):
            break
        path = nxt[len(API):]
    return rows, included


def build_names(aid):
    """build id -> "1.2 (34)", for the app's newest builds (version of the prerelease, build number)."""
    rows, included = pages(f"/v1/builds?filter[app]={q(aid)}&sort=-uploadedDate&limit=200"
                           "&fields[builds]=version,uploadedDate,preReleaseVersion&include=preReleaseVersion"
                           "&fields[preReleaseVersions]=version", limit_pages=1)
    versions = {i["id"]: (i.get("attributes") or {}).get("version") for i in included if i.get("type") == "preReleaseVersions"}
    names = {}
    for b in rows:
        pre = (((b.get("relationships") or {}).get("preReleaseVersion") or {}).get("data") or {}).get("id")
        number = (b.get("attributes") or {}).get("version") or "?"
        names[b["id"]] = f"{versions.get(pre) or '?'} ({number})"
    return names, rows


def _ips(text):
    """An .ips crash report (one JSON header line, then one JSON body) as (header, body), or None if it is not one."""
    text = text.strip()
    if not text.startswith("{"):
        return None
    head, _, rest = text.partition("\n")
    try:
        header = json.loads(head)
        body = json.loads(rest) if rest.strip() else {}
    except ValueError:
        try:
            return {}, json.loads(text)
        except ValueError:
            return None
    if not rest.strip():
        # one JSON object only: the body itself
        return {}, header
    return header, body


def crash_digest(text):
    """The lines that say what crashed: exception, termination reason, the crashed thread and its first frames.
    Reads the text form (Exception Type:, Termination Reason:, Thread N Crashed:) and the .ips JSON form."""
    ips = _ips(text)
    if ips is not None:
        header, body = ips
        out = []
        exc = body.get("exception") or {}
        if exc:
            out.append("Exception Type: " + " ".join(str(x) for x in (exc.get("type"), f"({exc['signal']})" if exc.get("signal") else None) if x))
            if exc.get("codes"):
                out.append(f"Exception Codes: {exc['codes']}")
        term = body.get("termination") or {}
        if term:
            reason = " ".join(str(term[k]) for k in ("namespace", "code") if term.get(k) is not None)
            if term.get("indicator"):
                reason += f" {term['indicator']}"
            out.append(f"Termination Reason: {reason.strip()}")
        asi = body.get("asi") or {}
        for lines in asi.values() if isinstance(asi, dict) else ():
            for line in (lines if isinstance(lines, list) else [lines])[:2]:
                out.append(f"Application Specific Information: {str(line)[:300]}")
        threads = body.get("threads") or []
        idx = body.get("faultingThread")
        if not isinstance(idx, int) or not 0 <= idx < len(threads):
            idx = next((i for i, t in enumerate(threads) if t.get("triggered")), None)
        if idx is not None:
            images = body.get("usedImages") or []
            out.append(f"Thread {idx} Crashed:" + (f" {threads[idx]['queue']}" if threads[idx].get("queue") else ""))
            for n, f in enumerate((threads[idx].get("frames") or [])[:FRAMES]):
                img = f.get("imageIndex")
                name = images[img].get("name", "?") if isinstance(img, int) and 0 <= img < len(images) else "?"
                sym = f.get("symbol") or f"+{f.get('imageOffset', '?')}"
                loc = f" + {f['symbolLocation']}" if f.get("symbolLocation") is not None and f.get("symbol") else ""
                src = f" ({f['sourceFile']}:{f.get('sourceLine', '?')})" if f.get("sourceFile") else ""
                out.append(f"{n:<3} {name:<24} {sym}{loc}{src}")
        meta = {"app_version": header.get("app_version"), "build_version": header.get("build_version"),
                "os_version": header.get("os_version")}
        return out, meta
    lines = text.splitlines()
    out, meta = [], {}
    for i, line in enumerate(lines):
        key = line.split(":", 1)[0].strip()
        if key in ("Exception Type", "Exception Codes", "Termination Reason", "Termination Description",
                   "Triggered by Thread", "Exception Reason"):
            out.append(line.strip()[:300])
        elif key == "Version" and "version" not in meta:
            meta["version"] = line.split(":", 1)[1].strip()
        elif re.match(r"^Thread \d+ Crashed\b", line):
            out.append(line.strip())
            taken = 0
            for frame in lines[i + 1:]:
                if not frame.strip() or taken >= FRAMES:
                    break
                if re.match(r"^\d+\s", frame):
                    out.append(re.sub(r"\s+", " ", frame.strip()))
                    taken += 1
    return out, meta


def _slug(text):
    return re.sub(r"[^0-9A-Za-z.-]+", "-", text).strip("-")[:40] or "x"


def crashes(out_dir, since=None):
    """TestFlight crash feedback since SINCE (default: 3 days ago) and the diagnostic signatures of the newest builds,
    written to OUT_DIR: one file per crash log, diagnostics/*.json, summary.md (with tester comments, for the artifact)
    and issue.md (without them, for a public issue). GITHUB_OUTPUT: crashes=N."""
    start = when(since) if since else datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=3)
    if start is None:
        die(f"not an ISO 8601 date: {since}")
    os.makedirs(os.path.join(out_dir, "diagnostics"), exist_ok=True)
    aid = app_id(os.environ["BUNDLE_ID"]) or die("no app record")
    names, recent = build_names(aid)

    # 1. Crash feedback, newest first, until one is older than SINCE
    found, path = [], (f"/v1/apps/{aid}/betaFeedbackCrashSubmissions?sort=-createdDate&limit=50"
                       f"&fields[betaFeedbackCrashSubmissions]={CRASH_FIELDS}")
    for _ in range(20):
        page = call("GET", path, ok=(400, 403, 404))
        if "error" in page:
            print(f"::warning::the crash feedback could not be read: HTTP {page['error']} {', '.join(page['codes'])} "
                  f"{page['detail']}", file=sys.stderr)
            break
        rows = page.get("data") or []
        older = False
        for row in rows:
            attrs = {k: v for k, v in (row.get("attributes") or {}).items() if k in KEEP}
            created = when(attrs.get("createdDate"))
            if created and created < start:
                older = True
                break
            build = (((row.get("relationships") or {}).get("build") or {}).get("data") or {}).get("id")
            found.append({"id": row["id"], "build": build, **attrs})
        nxt = (page.get("links") or {}).get("next")
        if older or not nxt or not nxt.startswith(API):
            break
        path = nxt[len(API):]

    for c in found:
        log = call("GET", f"/v1/betaFeedbackCrashSubmissions/{c['id']}/crashLog?fields[betaCrashLogs]=logText",
                   ok=(400, 403, 404))
        text = ((log.get("data") or {}).get("attributes") or {}).get("logText") or ""
        if "error" in log or not text:
            c["digest"], c["meta"], c["file"] = ["(no crash log: " + (f"HTTP {log['error']}" if "error" in log else "empty") + ")"], {}, None
            continue
        ext = "ips" if _ips(text) is not None else "crash"
        c["file"] = f"crash-{_slug(c.get('createdDate') or '')}-{_slug(c['id'])[:12]}.{ext}"
        with open(os.path.join(out_dir, c["file"]), "w", encoding="utf-8") as f:
            f.write(text)
        c["digest"], c["meta"] = crash_digest(text)

    # 2. Diagnostic signatures (hangs, launches, disk writes) of the newest builds
    diags = []
    for b in recent[:3]:
        for kind in DIAGNOSTIC_TYPES:
            sigs, _ = pages(f"/v1/builds/{b['id']}/diagnosticSignatures?filter[diagnosticType]={kind}&limit=50", limit_pages=2)
            for sig in sigs:
                a = sig.get("attributes") or {}
                logs = call("GET", f"/v1/diagnosticSignatures/{sig['id']}/logs?limit=5", ok=(400, 403, 404))
                frames = []
                if "error" not in logs:
                    name = f"{_slug(names.get(b['id'], b['id']))}-{kind.lower()}-{_slug(sig['id'])[:12]}.json"
                    with open(os.path.join(out_dir, "diagnostics", name), "w", encoding="utf-8") as f:
                        json.dump(logs, f, indent=1)
                    frames = blame_frames(logs)
                diags.append({"build": names.get(b["id"], b["id"]), "type": a.get("diagnosticType") or kind,
                              "signature": a.get("signature") or "", "weight": a.get("weight"), "frames": frames})

    for name, private in (("summary.md", True), ("issue.md", False)):
        with open(os.path.join(out_dir, name), "w", encoding="utf-8") as f:
            f.write(crash_report(found, diags, names, start, private))
    print(f"{len(found)} crash report(s) since {start.isoformat()}, {len(diags)} diagnostic signature(s) of "
          f"{min(len(recent), 3)} build(s); written to {out_dir}")
    gh_out("crashes", len(found))


def blame_frames(logs):
    """The first frames of the first call stack of a diagnostic log, the blamed one marked."""
    out = []
    for product in logs.get("productData") or []:
        for log in product.get("diagnosticLogs") or []:
            for tree in log.get("callStackTree") or []:
                for stack in tree.get("callStacks") or []:
                    node = (stack.get("callStackRootFrames") or [None])[0]
                    while node and len(out) < FRAMES:
                        mark = " (blamed)" if node.get("isBlameFrame") else ""
                        out.append(f"{node.get('binaryName') or '?'} {node.get('symbolName') or node.get('rawFrame') or '?'}{mark}")
                        node = (node.get("subFrames") or [None])[0]
                    if out:
                        return out
    return out


def crash_report(found, diags, names, start, private):
    lines = [f"# TestFlight crashes since {start.strftime('%Y-%m-%d %H:%M UTC')}", ""]
    if not private:
        lines += ["Tester comments are left out here (the repository is public); they are in the run's artifact.", ""]
    if not found:
        lines += ["No crash feedback in this time.", ""]
    for c in found:
        meta = c.get("meta") or {}
        build = names.get(c.get("build")) or (f"{meta['app_version']} ({meta['build_version']})"
                                               if meta.get("app_version") else meta.get("version")) or "?"
        lines += [f"## {c.get('createdDate') or '?'}: build {build}", "",
                  f"- Device: {c.get('deviceModel') or '?'}, {c.get('osVersion') or '?'}"
                  + (f", {c['architecture']}" if c.get("architecture") else "")]
        if private and c.get("comment"):
            lines.append("- Tester comment: " + " ".join(c["comment"].split())[:1000])
        if c.get("file"):
            lines.append(f"- Log: `{c['file']}`")
        lines += ["", "```", *(c.get("digest") or ["(nothing recognised in the log)"]), "```", ""]
    if diags:
        lines += ["## Diagnostics of the newest builds", ""]
        for d in diags:
            weight = f", weight {d['weight']}" if isinstance(d.get("weight"), (int, float)) else ""
            lines.append(f"- {d['build']} {d['type']}{weight}: `{d['signature'][:200]}`")
            for fr in d["frames"]:
                lines.append(f"  - `{fr[:200]}`")
        lines.append("")
    return "\n".join(lines)


def gh_out(name, value):
    print(f"{name}: {value}")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as f:
            f.write(f"{name}={value}\n")


if __name__ == "__main__":
    cmds = {"prepare": prepare, "wait": wait, "upload": upload, "internal": internal, "notes": notes, "next-build": next_build,
            "last-sha": last_sha, "export-code": export_code, "external": external, "beta-review": beta_review,
            "whats-new": whats_new, "crashes": crashes}
    if len(sys.argv) < 2 or sys.argv[1] not in cmds:
        sys.exit(__doc__)
    cmds[sys.argv[1]](*sys.argv[2:])
