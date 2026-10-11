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
