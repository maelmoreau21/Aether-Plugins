"""Packaging must retain the signed migration declaration for private plugins."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class PackageManifestTest(unittest.TestCase):
    def test_signed_migration_is_preserved_and_provider_fields_are_excluded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            migration = {"fromVersion": "0.1.0", "keyRenames": [{"from": "old", "to": "new"}]}
            source = {"apiVersion": 1, "id": "test.private", "name": "Private",
                      "description": "Synthetic", "author": "Test", "license": "MIT",
                      "permissions": ["metadata.read", "storage.private"],
                      "storageMigration": migration, "provider": "excluded"}
            (root / "source.json").write_text(json.dumps(source), encoding="utf-8")
            (root / "module.wasm").write_bytes(b"synthetic module")
            subprocess.run([sys.executable, str(Path(__file__).with_name("package-manifest.py")),
                            "--input", str(root / "source.json"), "--plugin-root", str(root),
                            "--version", "0.1.1", "--wasm", str(root / "module.wasm"),
                            "--repository", "https://example.invalid/repo", "--output", str(root / "out.json")], check=True)
            result = json.loads((root / "out.json").read_text(encoding="utf-8"))
            self.assertEqual(result["storageMigration"], migration)
            self.assertEqual(result["version"], "0.1.1")
            self.assertEqual(result["checksum"], hashlib.sha256(b"synthetic module").hexdigest())
            self.assertNotIn("provider", result)


if __name__ == "__main__":
    unittest.main()
