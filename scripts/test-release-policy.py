import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("detect_changed_plugins.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

def fake_git(args, cwd=None):
    if args[0] == "diff":
        return "Aether-Plugins-IMDb/src/lib.rs\nAether-Plugins-TMDb/src/lib.rs"
    if args[0] == "rev-parse" and args[1].startswith("tmdb-v"):
        raise RuntimeError("tag absent")
    return ""

class ReleasePolicyTests(unittest.TestCase):
    def test_manual_release_changes_only_selected_plugin(self):
        for bump, expected in [("PATCH", "1.2.4"), ("MINOR", "1.3.0"), ("MAJOR", "2.0.0")]:
            with self.subTest(bump=bump), patch.object(sys, "argv", ["release", "--manual-plugin", "tmdb", "--manual-bump", bump]), patch.object(release, "get_current_cargo_version", return_value="1.2.3"), patch.object(release, "run_git", side_effect=fake_git), contextlib.redirect_stdout(io.StringIO()) as output:
                release.main()
                entries = json.loads(output.getvalue())
                self.assertEqual(len(entries), 1)
                self.assertEqual(entries[0]["id"], "tmdb")
                self.assertEqual(entries[0]["next_version"], expected)

if __name__ == "__main__":
    unittest.main()
