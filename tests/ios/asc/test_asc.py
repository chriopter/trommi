"""The App Store Connect steps of the public beta (asc.py external, beta-review, whats-new), dry: every API call is
answered from fixtures/api.json and recorded, nothing goes to the network and no key is needed.

    python3 -m unittest discover -s tests/ios/asc -v
"""
import contextlib, copy, importlib.util, io, json, os, pathlib, sys, unittest

sys.dont_write_bytecode = True
HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
spec = importlib.util.spec_from_file_location("asc", ROOT / "app/ios/TrommiApp/AppStore/asc.py")
asc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(asc)
API = json.loads((HERE / "fixtures/api.json").read_text())


class Fake:
    """Stands in for asc.call: answers by "METHOD path", records every call."""

    def __init__(self, **over):
        self.answers = {**copy.deepcopy(API), **over}
        self.sent = []

    def __call__(self, method, path, body=None, ok=()):
        self.sent.append((method, path, body))
        key = f"{method} {path}"
        if key not in self.answers:
            raise AssertionError(f"unexpected call {key}")
        return copy.deepcopy(self.answers[key])

    def writes(self):
        return [(m, p, b) for m, p, b in self.sent if m != "GET"]


class Base(unittest.TestCase):
    def setUp(self):
        self.env = {k: os.environ.get(k) for k in ("BUNDLE_ID", "APPSTORE_DEMO_PASSWORD", "GITHUB_OUTPUT")}
        os.environ["BUNDLE_ID"] = "com.trommi.ios"
        os.environ["APPSTORE_DEMO_PASSWORD"] = "fixture-password"
        os.environ.pop("GITHUB_OUTPUT", None)
        self.real = asc.call

    def tearDown(self):
        asc.call = self.real
        for k, v in self.env.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v

    def run_with(self, fake, fn, *args):
        asc.call = fake
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                fn(*args)
                code = 0
            except SystemExit as e:
                code = e.code
        return code, out.getvalue(), err.getvalue()


class External(Base):
    def test_adds_the_build_and_prints_the_public_link(self):
        fake = Fake()
        code, out, _ = self.run_with(fake, asc.external, "b1", "Public")
        self.assertEqual(code, 0)
        self.assertEqual(fake.writes(), [("POST", "/v1/betaGroups/g-public/relationships/builds",
                                          {"data": [{"type": "builds", "id": "b1"}]})])
        self.assertIn("public_link: https://testflight.apple.com/join/AbCd1234", out)

    def test_a_missing_group_fails_telling_the_owner_to_make_it(self):
        fake = Fake(**{"GET /v1/apps/app1/betaGroups?limit=200": {"data": API["GET /v1/apps/app1/betaGroups?limit=200"]["data"][:1]}})
        code, _, err = self.run_with(fake, asc.external, "b1", "Public")
        self.assertEqual(code, 1)
        self.assertIn('no TestFlight group "Public"', err)
        self.assertIn("External Testing", err)
        self.assertEqual(fake.writes(), [])

    def test_an_internal_group_of_that_name_is_refused(self):
        fake = Fake()
        code, _, err = self.run_with(fake, asc.external, "b1", "Intern")
        self.assertEqual(code, 1)
        self.assertIn("internal group", err)
        self.assertEqual(fake.writes(), [])


class BetaReview(Base):
    def test_first_build_of_a_version_is_submitted(self):
        fake = Fake()
        code, out, _ = self.run_with(fake, asc.beta_review, "b1")
        self.assertEqual(code, 0)
        writes = fake.writes()
        detail = writes[0]
        self.assertEqual(detail[1], "/v1/betaAppReviewDetails/rd1")
        attrs = detail[2]["data"]["attributes"]
        self.assertEqual(attrs["demoAccountName"], "appreview@trommi.com")
        self.assertEqual(attrs["demoAccountPassword"], "fixture-password")
        self.assertTrue(attrs["demoAccountRequired"])
        self.assertEqual(attrs["contactEmail"], "trommi@mail101.de")
        self.assertNotIn("contactPhone", attrs)  # empty in the repository: what App Store Connect has stays
        self.assertNotIn("fixture-password", out)
        # the empty description is filled, the address that is there is left alone
        self.assertEqual(writes[1][1], "/v1/betaAppLocalizations/loc1")
        self.assertEqual(set(writes[1][2]["data"]["attributes"]), {"description"})
        self.assertEqual(writes[2], ("POST", "/v1/betaAppReviewSubmissions", {"data": {
            "type": "betaAppReviewSubmissions", "relationships": {"build": {"data": {"type": "builds", "id": "b1"}}}}}))
        self.assertIn("external_state: READY_FOR_BETA_SUBMISSION", out)

    def test_a_build_already_submitted_is_not_sent_again(self):
        fake = Fake(**{"GET /v1/betaAppReviewSubmissions?filter[build]=b1&limit=5": {"data": [
            {"id": "sub0", "attributes": {"betaReviewState": "APPROVED"}}]}})
        code, out, _ = self.run_with(fake, asc.beta_review, "b1")
        self.assertEqual(code, 0)
        self.assertNotIn("/v1/betaAppReviewSubmissions", [p for m, p, _ in fake.writes()])
        self.assertIn("already submitted (APPROVED)", out)

    def test_a_later_build_of_a_reviewed_version_needs_none(self):
        fake = Fake(**{"GET /v1/builds/b1/buildBetaDetail": {"data": {"id": "bd1", "attributes": {
            "externalBuildState": "IN_BETA_TESTING"}}}})
        code, out, _ = self.run_with(fake, asc.beta_review, "b1")
        self.assertEqual(code, 0)
        self.assertNotIn("/v1/betaAppReviewSubmissions", [p for m, p, _ in fake.writes()])
        self.assertIn("no Beta App Review needed now (IN_BETA_TESTING)", out)

    def test_without_the_demo_password_nothing_is_sent(self):
        os.environ.pop("APPSTORE_DEMO_PASSWORD")
        fake = Fake()
        code, _, err = self.run_with(fake, asc.beta_review, "b1")
        self.assertEqual(code, 1)
        self.assertIn("APPSTORE_DEMO_PASSWORD is not set", err)
        self.assertEqual(fake.writes(), [])

    def test_no_phone_anywhere_fails_before_a_submission(self):
        fake = Fake(**{"GET /v1/apps/app1/betaAppReviewDetail": {"data": {"id": "rd1", "attributes": {}}}})
        code, _, err = self.run_with(fake, asc.beta_review, "b1")
        self.assertEqual(code, 1)
        self.assertIn("phone number", err)
        self.assertEqual(fake.writes(), [])


    def test_another_build_in_review_leaves_this_one_waiting_without_failing(self):
        def fake_with_422(method, path, body=None, ok=()):
            if (method, path) == ("POST", "/v1/betaAppReviewSubmissions"):
                fake.sent.append((method, path, body))
                self.assertIn(422, ok)
                return {"error": 422, "codes": ["ENTITY_UNPROCESSABLE.ANOTHER_BUILD_IN_REVIEW"],
                        "detail": "Another build in the same train is already in beta review."}
            return fake(method, path, body, ok)
        fake = Fake(**{
            "GET /v1/builds/b1?fields[builds]=version": {"data": {"id": "b1", "attributes": {"version": "60"}}},
            "GET /v1/builds?filter[app]=app1&filter[betaAppReviewSubmission.betaReviewState]=WAITING_FOR_REVIEW,IN_REVIEW&fields[builds]=version&limit=5":
                {"data": [{"id": "b0", "attributes": {"version": "59"}}]}})
        code, out, err = self.run_with(fake_with_422, asc.beta_review, "b1")
        self.assertEqual(code, 0, err)
        self.assertIn("build 60 waits; build 59 is in review", out)
        self.assertIn("external_state: WAITING_FOR_ANOTHER_REVIEW", out)
        # the build is not taken out of a group
        self.assertFalse([w for w in fake.writes() if w[0] == "DELETE"])

    def test_another_422_still_fails(self):
        def fake_with_422(method, path, body=None, ok=()):
            if (method, path) == ("POST", "/v1/betaAppReviewSubmissions"):
                return {"error": 422, "codes": ["ENTITY_UNPROCESSABLE.SOMETHING_ELSE"], "detail": "no"}
            return fake(method, path, body, ok)
        fake = Fake()
        code, _, err = self.run_with(fake_with_422, asc.beta_review, "b1")
        self.assertEqual(code, 1)
        self.assertIn("SOMETHING_ELSE", err)


class Crashes(Base):
    """asc.py crashes: the crash feedback of TestFlight and the diagnostics of the newest builds, read only."""
    LIST = ("GET /v1/apps/app1/betaFeedbackCrashSubmissions?sort=-createdDate&limit=50"
            f"&fields[betaFeedbackCrashSubmissions]={asc.CRASH_FIELDS}")
    BUILDS = ("GET /v1/builds?filter[app]=app1&sort=-uploadedDate&limit=200&fields[builds]=version,uploadedDate,"
              "preReleaseVersion&include=preReleaseVersion&fields[preReleaseVersions]=version")

    def answers(self):
        sub = lambda i, date, build, comment: {"id": i, "type": "betaFeedbackCrashSubmissions", "attributes": {
            "createdDate": date, "comment": comment, "email": "tester@example.com", "deviceModel": "iPhone17,1",
            "osVersion": "27.0"}, "relationships": {"build": {"data": {"type": "builds", "id": build}}}}
        log = lambda name: {"data": {"id": "l", "type": "betaCrashLogs",
                                     "attributes": {"logText": (HERE / "fixtures" / name).read_text()}}}
        nodiag = {f"GET /v1/builds/{b}/diagnosticSignatures?filter[diagnosticType]={k}&limit=50": {"data": []}
                  for b in ("b2", "b1") for k in asc.DIAGNOSTIC_TYPES}
        return {
            **nodiag,
            self.BUILDS: {"data": [
                {"id": "b2", "attributes": {"version": "62"}, "relationships": {"preReleaseVersion": {"data": {"id": "p1"}}}},
                {"id": "b1", "attributes": {"version": "61"}, "relationships": {"preReleaseVersion": {"data": {"id": "p1"}}}}],
                "included": [{"type": "preReleaseVersions", "id": "p1", "attributes": {"version": "1.0"}}]},
            self.LIST: {"data": [sub("c3", "2026-10-11T08:00:00Z", "b2", "it crashed when I opened\nthe board"),
                                 sub("c2", "2026-10-10T08:00:00.123Z", "b1", ""),
                                 sub("c1", "2026-10-09T08:00:00Z", "b9", ""),
                                 sub("c0", "2026-10-01T08:00:00Z", "b1", "too old")],
                        "links": {"next": asc.API + "/v1/never-asked"}},
            "GET /v1/betaFeedbackCrashSubmissions/c3/crashLog?fields[betaCrashLogs]=logText": log("crash.ips"),
            "GET /v1/betaFeedbackCrashSubmissions/c2/crashLog?fields[betaCrashLogs]=logText": log("crash.txt"),
            "GET /v1/betaFeedbackCrashSubmissions/c1/crashLog?fields[betaCrashLogs]=logText": {"error": 404, "codes": ["NOT_FOUND"], "detail": ""},
            "GET /v1/builds/b2/diagnosticSignatures?filter[diagnosticType]=HANGS&limit=50": {"data": [
                {"id": "s1", "attributes": {"diagnosticType": "HANGS", "signature": "Sync.apply(_:)", "weight": 0.25}}]},
            "GET /v1/builds/b1/diagnosticSignatures?filter[diagnosticType]=DISK_WRITES&limit=50": {"error": 403, "codes": ["FORBIDDEN"], "detail": ""},
            "GET /v1/diagnosticSignatures/s1/logs?limit=5": {"productData": [{"signatureId": "s1", "diagnosticLogs": [
                {"callStackTree": [{"callStacks": [{"callStackRootFrames": [{"binaryName": "Trommi", "symbolName": "main",
                 "subFrames": [{"binaryName": "Trommi", "symbolName": "Sync.apply(_:)", "isBlameFrame": True}]}]}]}]}]}]},
        }

    def test_writes_logs_and_a_summary_without_e_mail(self):
        import tempfile
        fake = Fake(**self.answers())
        with tempfile.TemporaryDirectory() as out:
            code, stdout, err = self.run_with(fake, asc.crashes, out, "2026-10-08")
            self.assertEqual(code, 0, err)
            self.assertIn("crashes: 3", stdout)
            files = sorted(os.listdir(out))
            self.assertEqual(len([f for f in files if f.endswith(".ips")]), 1)
            self.assertEqual(len([f for f in files if f.endswith(".crash")]), 1)
            self.assertEqual(len(os.listdir(os.path.join(out, "diagnostics"))), 1)
            summary = pathlib.Path(out, "summary.md").read_text()
            issue = pathlib.Path(out, "issue.md").read_text()
        for text in (summary, issue):
            self.assertNotIn("tester@example.com", text)
            self.assertNotIn("too old", text)
            self.assertIn("## 2026-10-11T08:00:00Z: build 1.0 (62)", text)
            self.assertIn("Exception Type: EXC_BAD_ACCESS (SIGSEGV)", text)
            self.assertIn("Termination Reason: SIGNAL 11 Segmentation fault: 11", text)
            self.assertIn("Thread 1 Crashed: com.trommi.sync", text)
            self.assertIn("Sync.apply(_:) + 120 (Sync.swift:42)", text)
            self.assertIn("Exception Type:  EXC_CRASH (SIGABRT)", text)
            self.assertIn("3 Trommi 0x0000000100a2b3c7 BoardView.body.getter + 412 (BoardView.swift:88)", text)
            self.assertNotIn("Thread 1:", text)
            self.assertIn("build ?", text)  # c1: build unknown, no log
            self.assertIn("(no crash log: HTTP 404)", text)
            self.assertIn("1.0 (62) HANGS, weight 0.25: `Sync.apply(_:)`", text)
            self.assertIn("Trommi Sync.apply(_:) (blamed)", text)
        self.assertIn("Tester comment: it crashed when I opened the board", summary)
        self.assertNotIn("it crashed", issue)
        self.assertNotIn("- Tester comment", issue)
        # never asks for the tester or the e-mail, writes nothing, does not page past SINCE
        for _, path, _ in fake.sent:
            self.assertNotIn("tester", path)
            self.assertNotIn("email", path)
        self.assertEqual(fake.writes(), [])
        self.assertNotIn("/v1/never-asked", [p for _, p, _ in fake.sent])

    def test_the_text_digest_keeps_eight_frames(self):
        lines, meta = asc.crash_digest((HERE / "fixtures/crash.txt").read_text())
        self.assertEqual(meta.get("version"), "1.0 (61)")
        self.assertEqual(len([l for l in lines if l[:1].isdigit()]), asc.FRAMES)

    def test_no_crashes_still_writes_a_summary(self):
        import tempfile
        over = self.answers()
        over[self.LIST] = {"data": [], "links": {}}
        with tempfile.TemporaryDirectory() as out:
            code, stdout, err = self.run_with(Fake(**over), asc.crashes, out)
            self.assertEqual(code, 0, err)
            self.assertIn("crashes: 0", stdout)
            self.assertIn("No crash feedback", pathlib.Path(out, "summary.md").read_text())


class WhatsNew(Base):
    def test_the_newest_day_as_plain_text(self):
        code, out, _ = self.run_with(None, asc.whats_new, str(HERE / "fixtures/CHANGELOG.md"))
        self.assertEqual(code, 0)
        self.assertEqual(out.strip(), "New:\n"
                         "- The iPhone app as a public beta: join on TestFlight.\n"
                         "- Drag sessions in the sidebar into your own order; the iPhone shows the same order.\n"
                         "Fixed:\n"
                         "- Log in no longer hangs.")

    def test_the_real_changelog_fits_what_to_test(self):
        code, out, _ = self.run_with(None, asc.whats_new, str(ROOT / "CHANGELOG.md"))
        self.assertEqual(code, 0)
        self.assertLessEqual(len(out.strip()), 3900)
        self.assertTrue(out.startswith(("New:", "Better:", "Fixed:")))


if __name__ == "__main__":
    unittest.main()
