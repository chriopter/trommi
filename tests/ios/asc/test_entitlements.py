"""ios_entitlements.py against the entitlements files: Xcode's $(AppIdentifierPrefix) read as the signing team, the
keychain group the app shares with its notification extension only, and none of it in the widget or the Mac app.

    python3 -m unittest discover -s tests/ios/asc -v
"""
import importlib.util, pathlib, plistlib, sys, unittest

sys.dont_write_bytecode = True
ROOT = pathlib.Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("ios_entitlements", ROOT / ".github/scripts/ios_entitlements.py")
ent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ent)
STORE = ROOT / "app/ios/TrommiApp/AppStore"
TEAM = "NL9YA3V25N"


def load(name):
    return plistlib.loads((STORE / name).read_bytes())


def signed_as(want, team):
    """What Xcode signs from a file: the prefix filled with the team and a dot (empty without a team)."""
    def fill(v):
        if isinstance(v, str):
            return v.replace(ent.PREFIX, f"{team}." if team else "")
        return [fill(x) for x in v] if isinstance(v, list) else v
    return {k: fill(v) for k, v in want.items()}


class Entitlements(unittest.TestCase):
    def holds(self, want, have, team):
        return all(ent.same(v, have.get(k), team) for k, v in want.items())

    def test_the_app_and_the_extension_share_the_keys_group(self):
        app, notify = load("TrommiApp.entitlements"), load("TrommiNotify.entitlements")
        self.assertEqual(app["keychain-access-groups"],
                         [f"{ent.PREFIX}com.trommi.ios", f"{ent.PREFIX}com.trommi.ios.notify-keys"],
                         "the app's own group first: an item added without a group stays the app's")
        self.assertEqual(notify["keychain-access-groups"], [f"{ent.PREFIX}com.trommi.ios.notify-keys"])

    def test_nothing_else_gets_a_keychain_group(self):
        for name in ["TrommiLive.entitlements", "TrommiShare.entitlements", "TrommiMac.entitlements"]:
            self.assertNotIn("keychain-access-groups", load(name), name)

    def test_the_prefix_is_the_signing_team(self):
        for name in ["TrommiApp.entitlements", "TrommiNotify.entitlements"]:
            want = load(name)
            self.assertTrue(self.holds(want, signed_as(want, TEAM), TEAM), name)
            self.assertTrue(self.holds(want, signed_as(want, TEAM), None), f"{name}: ad hoc, the team not named")
            self.assertTrue(self.holds(want, signed_as(want, None), None), f"{name}: ad hoc without a team")
            self.assertFalse(self.holds(want, signed_as(want, "AAAAAAAAAA"), TEAM), f"{name}: another team")
            self.assertFalse(self.holds(want, signed_as(want, None), TEAM), f"{name}: a team named, no prefix")

    def test_order_and_values_still_count(self):
        want = [f"{ent.PREFIX}com.trommi.ios", f"{ent.PREFIX}com.trommi.ios.notify-keys"]
        self.assertFalse(ent.same(want, [f"{TEAM}.com.trommi.ios.notify-keys", f"{TEAM}.com.trommi.ios"], TEAM))
        self.assertFalse(ent.same(want, [f"{TEAM}.com.trommi.ios"], TEAM))
        self.assertTrue(ent.same(["group.com.trommi.ios"], ["group.com.trommi.ios"], TEAM))
        self.assertFalse(ent.same(True, False, TEAM))

    def test_the_mac_app_file_still_matches_itself(self):
        want = load("TrommiMac.entitlements")
        self.assertTrue(self.holds(want, dict(want), TEAM))


if __name__ == "__main__":
    unittest.main()
