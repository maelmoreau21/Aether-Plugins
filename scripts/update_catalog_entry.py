#!/usr/bin/env python3
"""
update_catalog_entry.py
Met à jour ou ajoute une entrée de plugin dans repository/manifest.json de manière atomique.
Conserve toutes les autres entrées, les signatures existantes et vérifie la taille du manifeste.

Usage:
  python update_catalog_entry.py \
    --package-path <path/to/plugin.package.json> \
    --public-key-path <path/to/publisher-public.txt> \
    --base-url <https://raw.githubusercontent.com/.../repository/packages/> \
    --manifest-path <path/to/repository/manifest.json>
"""

import sys
import os
import json
import hashlib
import argparse

def compute_sha256(filepath):
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest().lower()

def main():
    parser = argparse.ArgumentParser(description="Atomically update plugin entry in repository/manifest.json")
    parser.add_argument("--package-path", required=True, help="Path to the signed package json file")
    parser.add_argument("--public-key-path", required=True, help="Path to the publisher public key text file")
    parser.add_argument("--base-url", required=True, help="Base URL where packages are hosted")
    parser.add_argument("--manifest-path", required=True, help="Path to repository/manifest.json")

    args = parser.parse_args()

    if not os.path.exists(args.package_path):
        raise FileNotFoundError(f"Package file not found: {args.package_path}")
    if not os.path.exists(args.public_key_path):
        raise FileNotFoundError(f"Public key file not found: {args.public_key_path}")
    if not os.path.exists(args.manifest_path):
        raise FileNotFoundError(f"Manifest file not found: {args.manifest_path}")

    with open(args.package_path, "r", encoding="utf-8") as f:
        pkg = json.load(f)

    if not pkg.get("manifest") or not pkg["manifest"].get("id"):
        raise ValueError("Invalid package: missing manifest.id")
    if not pkg.get("signature"):
        raise ValueError("Invalid package: missing signature")

    with open(args.public_key_path, "r", encoding="utf-8") as f:
        public_key = f.read().strip()

    package_filename = os.path.basename(args.package_path)
    package_sha256 = compute_sha256(args.package_path)
    base_url = args.base_url.rstrip("/") + "/"
    package_url = f"{base_url}{package_filename}"

    new_entry = {
        "manifest": pkg["manifest"],
        "publisherId": pkg.get("publisherId", "maelmoreau21"),
        "publicKey": public_key,
        "packageUrl": package_url,
        "packageSha256": package_sha256
    }

    plugin_id = pkg["manifest"]["id"]

    with open(args.manifest_path, "r", encoding="utf-8") as f:
        manifest_data = json.load(f)

    extensions = manifest_data.get("extensions", [])
    updated = False
    new_extensions = []

    for ext in extensions:
        if ext.get("manifest", {}).get("id") == plugin_id:
            new_extensions.append(new_entry)
            updated = True
        else:
            new_extensions.append(ext)

    if not updated:
        new_extensions.append(new_entry)

    # Trier par ordre alphabétique de nom ou d'id pour un diff propre
    new_extensions.sort(key=lambda x: x.get("manifest", {}).get("id", ""))

    manifest_data["apiVersion"] = 1
    manifest_data["extensions"] = new_extensions

    output_str = json.dumps(manifest_data, indent=2, ensure_ascii=False)
    if len(output_str.encode("utf-8")) > 262144:
        raise ValueError("Manifest size exceeds hard limit of 256 KiB!")

    # Écriture atomique
    temp_path = f"{args.manifest_path}.tmp"
    with open(temp_path, "w", encoding="utf-8") as f:
        f.write(output_str + "\n")
    os.replace(temp_path, args.manifest_path)

    action = "Updated" if updated else "Added"
    print(f"Successfully {action} extension '{plugin_id}' version {pkg['manifest']['version']} in {args.manifest_path}")

if __name__ == "__main__":
    main()
