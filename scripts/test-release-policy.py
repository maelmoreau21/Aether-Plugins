#!/usr/bin/env python3
"""
test-release-policy.py
Comprehensive test suite validating independent plugin release policies for Aether-Plugins:
13. Modification d'un seul plugin (les autres ne sont pas impactés)
14. Modification d'une dépendance commune aux plugins (crates/aether-plugin-api)
15. Modification documentaire sans publication inutile
17. Refus d'écraser un paquet signé existant (immutabilité)
18. Respect absolu des protections de branches (aucun push direct sur main dans les workflows de release)
+ Incrément bêta sur develop
"""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("detect_changed_plugins.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

class PluginsReleasePolicyTests(unittest.TestCase):

    def run_detect(self, args_list, fake_diff="", head_tags=None, all_tags=None, pkg_exists=False):
        def fake_git(args, cwd=None):
            if args[0] == "diff":
                return fake_diff
            if args[0] == "tag" and "--points-at" in args:
                return "\n".join(head_tags or [])
            if args[0] == "tag" and "-l" in args:
                return "\n".join(all_tags or [])
            if args[0] == "rev-parse":
                return "11223344556677889900"
            return ""

        real_exists = os.path.exists
        def fake_exists(p):
            if str(p).endswith(".package.json"):
                return pkg_exists
            return real_exists(p)

        with patch.object(sys, "argv", ["detect_changed_plugins.py"] + args_list), \
             patch.object(release, "run_git", side_effect=fake_git), \
             patch("os.path.exists", side_effect=fake_exists), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            release.main()
            return json.loads(output.getvalue())

    # 13. Modification d'un seul plugin : TMDb seul change, pas les autres
    def test_13_single_plugin_change_only_bumps_that_plugin(self):
        diff = "Aether-Plugins-TMDb/src/client.rs\nAether-Plugins-TMDb/src/lib.rs"
        entries = self.run_detect(["--channel", "main"], fake_diff=diff)
        triggered_ids = [e["id"] for e in entries if e["reason"].startswith("Source changes")]
        self.assertEqual(triggered_ids, ["tmdb"])
        self.assertNotIn("imdb", triggered_ids)
        self.assertNotIn("omdb", triggered_ids)

    # 14. Modification de la dépendance commune crates/aether-plugin-api
    def test_14_shared_dependency_change_detected(self):
        diff = "crates/aether-plugin-api/src/manifest.rs"
        entries = self.run_detect(["--channel", "main"], fake_diff=diff)
        shared_triggers = [e["id"] for e in entries if "Shared aether-plugin-api changed" in e["reason"]]
        self.assertTrue(len(shared_triggers) > 0)

    # 15. Modification documentaire sans publication inutile
    def test_15_documentation_only_change_no_unnecessary_release(self):
        diff = "Aether-Plugins-TMDb/README.md\nAether-Plugins-TMDb/docs/SPEC.txt\ncrates/aether-plugin-api/README.md"
        entries = self.run_detect(["--channel", "main"], fake_diff=diff)
        doc_triggers = [e for e in entries if e["reason"].startswith("Source changes") or "Shared" in e["reason"]]
        self.assertEqual(len(doc_triggers), 0)

    # 17. Refus d'écraser un paquet existant (immutabilité)
    def test_17_refusal_to_modify_existing_signed_package(self):
        diff = "Aether-Plugins-TMDb/src/lib.rs"
        entries = self.run_detect(["--channel", "main"], fake_diff=diff, pkg_exists=True)
        tmdb_entry = next((e for e in entries if e["id"] == "tmdb"), None)
        self.assertIsNotNone(tmdb_entry)
        self.assertTrue(tmdb_entry["package_already_exists"])

    # 18. Respect des protections de branches (Pas de push direct sur main)
    def test_18_branch_protections_no_direct_push_to_main(self):
        release_wf = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "release-plugins.yml"
        self.assertTrue(release_wf.exists())
        content = release_wf.read_text(encoding="utf-8")
        # Ensure git push origin main is NEVER invoked in release-plugins
        self.assertNotIn("git push origin main", content)
        # Git tags are pushed
        self.assertIn('git push origin "$TAG"', content)

        # Ensure preparation workflow uses Pull Requests
        prepare_wf = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "prepare-plugin-release.yml"
        self.assertTrue(prepare_wf.exists())
        prep_content = prepare_wf.read_text(encoding="utf-8")
        self.assertIn("gh pr create", prep_content)

    # Sur develop : incrément bêta
    def test_develop_channel_increments_beta(self):
        diff = "Aether-Plugins-TMDb/src/lib.rs"
        tags = ["tmdb-v0.2.0-beta.0"]
        entries = self.run_detect(["--channel", "develop"], fake_diff=diff, all_tags=tags)
        tmdb_entry = next((e for e in entries if e["id"] == "tmdb"), None)
        self.assertIsNotNone(tmdb_entry)
        self.assertEqual(tmdb_entry["next_version"], "0.2.0-beta.1")
        self.assertEqual(tmdb_entry["tag"], "tmdb-v0.2.0-beta.1")

    # Manuel : dispatch spécifique pour un plugin
    def test_manual_release_changes_only_selected_plugin(self):
        for bump, expected in [("PATCH", "1.2.4"), ("MINOR", "1.3.0"), ("MAJOR", "2.0.0")]:
            with self.subTest(bump=bump):
                with patch.object(release, "get_current_cargo_version", return_value="1.2.3"):
                    entries = self.run_detect(["--manual-plugin", "tmdb", "--manual-bump", bump, "--channel", "main"])
                    self.assertEqual(len(entries), 1)
                    self.assertEqual(entries[0]["id"], "tmdb")
                    self.assertEqual(entries[0]["next_version"], expected)

    # Manuel : dispatch pour tous les plugins (all)
    def test_manual_release_all_plugins(self):
        for bump, expected in [("PATCH", "1.2.4"), ("MINOR", "1.3.0"), ("MAJOR", "2.0.0")]:
            with self.subTest(bump=bump):
                with patch.object(release, "get_current_cargo_version", return_value="1.2.3"):
                    entries = self.run_detect(["--manual-plugin", "all", "--manual-bump", bump, "--channel", "main"])
                    self.assertEqual(len(entries), len(release.PLUGINS_MAP))
                    for entry in entries:
                        self.assertEqual(entry["next_version"], expected)
                        self.assertEqual(entry["bump_type"], bump)

if __name__ == "__main__":
    unittest.main()
