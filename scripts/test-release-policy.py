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

    # 18. Une seule publication simple : release GitHub + catalogue versionné.
    def test_18_single_release_workflow_without_branch_rewrites(self):
        workflows = Path(__file__).resolve().parent.parent / ".github" / "workflows"
        content = (workflows / "release.yml").read_text(encoding="utf-8")
        # No commit back to develop, no pull request, no per-branch URL rewriting.
        self.assertNotIn("HEAD:develop", content)
        self.assertNotIn("gh pr create", content)
        self.assertNotIn("/develop/repository/", content)
        # Write access is granted to the job only; assets are never replaced.
        self.assertIn("permissions: {}", content)
        self.assertIn("contents: write", content)
        self.assertNotIn("--clobber", content)
        # The catalogue keeps history through the append-only script.
        self.assertIn("add-catalogue-version.cjs", content)
        # Legacy publication workflows are gone: release.yml is the only publisher.
        for legacy in ("release-plugins.yml", "publish-develop-plugins.yml", "prepare-plugin-release.yml"):
            self.assertFalse((workflows / legacy).exists(), legacy)
        self.assertNotIn("publish-develop-plugins.yml", (workflows / "ci.yml").read_text(encoding="utf-8"))

    # 19. Aucune publication automatique sans que les contrôles CI réussissent.
    def test_19_auto_release_is_gated_on_ci_checks(self):
        workflows = Path(__file__).resolve().parent.parent / ".github" / "workflows"
        ci = (workflows / "ci.yml").read_text(encoding="utf-8")
        auto = (workflows / "auto-release.yml").read_text(encoding="utf-8")

        def job(name):
            block, inside = [], False
            for line in auto.splitlines():
                if line == f"  {name}:":
                    inside = True
                elif inside and line.startswith("  ") and not line.startswith("   ") and line.strip().endswith(":"):
                    break
                if inside:
                    block.append(line)
            return "\n".join(block)

        # ci.yml is reusable, so auto-release.yml runs the very same checks.
        self.assertIn("\n  workflow_call:", ci)
        checks = job("checks")
        self.assertIn("uses: ./.github/workflows/ci.yml", checks)
        # The caller grants what the called jobs request and nothing more.
        self.assertIn("contents: read", checks)
        self.assertNotIn("contents: write", checks)
        # The publishing job cannot start before the checks have succeeded.
        self.assertIn("needs: [plan, checks]", job("release"))
        self.assertNotIn("always()", job("release"))

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
    def test_manual_release_new_optional_providers(self):
        for plugin_id in ["tvdb", "opensubtitles"]:
            with self.subTest(plugin_id=plugin_id):
                entries = self.run_detect(["--manual-plugin", plugin_id, "--manual-bump", "PATCH", "--channel", "main"])
                self.assertEqual([entry["id"] for entry in entries], [plugin_id])

    def test_manual_release_all_plugins(self):
        for bump, expected in [("PATCH", "1.2.4"), ("MINOR", "1.3.0"), ("MAJOR", "2.0.0")]:
            with self.subTest(bump=bump):
                with patch.object(release, "get_current_cargo_version", return_value="1.2.3"):
                    entries = self.run_detect(["--manual-plugin", "all", "--manual-bump", bump, "--channel", "main"])
                    self.assertEqual(len(entries), len(release.PLUGINS_MAP))
                    for entry in entries:
                        self.assertEqual(entry["next_version"], expected)
                        self.assertEqual(entry["bump_type"], bump)

    # 20. Compilateur Rust épinglé, identique en local, en CI et en release.
    def test_20_rust_toolchain_is_pinned_everywhere(self):
        import re
        root = Path(__file__).resolve().parent.parent
        pin = (root / "rust-toolchain.toml").read_text(encoding="utf-8")
        channel = re.search(r'(?m)^channel = "(\d+\.\d+\.\d+)"$', pin)
        self.assertIsNotNone(channel, "channel must be an exact x.y.z version, not stable")
        for target in ("wasm32-unknown-unknown",):
            self.assertIn(target, pin)
        for component in ("rustfmt", "clippy"):
            self.assertIn(component, pin)
        # The workspace MSRV cannot be newer than the compiler used to build.
        msrv = re.search(r'(?m)^rust-version = "(\d+)\.(\d+)"$', (root / "Cargo.toml").read_text(encoding="utf-8"))
        self.assertIsNotNone(msrv)
        major, minor, _ = (int(part) for part in channel.group(1).split("."))
        self.assertLessEqual((int(msrv.group(1)), int(msrv.group(2))), (major, minor))
        # No workflow floats on `stable`: they all install what the file pins.
        for name in ("ci.yml", "release.yml"):
            workflow = (root / ".github" / "workflows" / name).read_text(encoding="utf-8")
            self.assertNotIn("rust-toolchain@", workflow, name)
            self.assertIn("rustup toolchain install", workflow, name)

if __name__ == "__main__":
    unittest.main()
