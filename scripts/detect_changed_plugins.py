#!/usr/bin/env python3
"""
detect_changed_plugins.py
Détecte les plugins modifiés entre un commit de référence (ou le dernier tag du plugin) et HEAD.
Règles :
- Analyse les chemins modifiés (`git diff --name-only <base>..HEAD`).
- Si un plugin (dossier Aether-Plugins-*) contient des modifications :
  - Ignore les modifications purement documentaires (.md, docs/) si aucun code source n'a changé.
- Si le crate commun `crates/aether-plugin-api` est modifié, tous les plugins dépendants sont marqués modifiés.
- Si `manual_plugin` et `manual_bump_type` (ex: MAJOR) sont fournis en argument, force la détection de ce plugin.

Sortie JSON lisible par les workflows GitHub Actions :
[
  {
    "id": "tmdb",
    "name": "aether-plugin-tmdb",
    "dir": "Aether-Plugins-TMDb",
    "current_version": "0.1.0",
    "next_version": "0.2.0",
    "tag": "tmdb-v0.2.0",
    "bump_type": "MINOR",
    "reason": "Source files changed in Aether-Plugins-TMDb"
  }
]
"""

import os
import sys
import json
import subprocess
import re

PLUGINS_MAP = [
    {
        "id": "avatars",
        "crate_name": "aether-plugin-avatar",
        "dir": "Aether-Plugins-Avatar",
        "tag_prefix": "avatar"
    },
    {
        "id": "discogs",
        "crate_name": "aether-plugin-discogs",
        "dir": "Aether-Plugins-Discogs",
        "tag_prefix": "discogs"
    },
    {
        "id": "fanart-tv",
        "crate_name": "aether-plugin-fanart",
        "dir": "Aether-Plugins-fanart.tv",
        "tag_prefix": "fanart"
    },
    {
        "id": "imdb",
        "crate_name": "aether-plugin-imdb",
        "dir": "Aether-Plugins-IMDb",
        "tag_prefix": "imdb"
    },
    {
        "id": "intro-skipper",
        "crate_name": "aether-plugin-intro-skipper",
        "dir": "Aether-Plugins-IntroSkipper",
        "tag_prefix": "intro-skipper"
    },
    {
        "id": "lrclib",
        "crate_name": "aether-plugin-lrclib",
        "dir": "Aether-Plugins-LrcLib",
        "tag_prefix": "lrclib"
    },
    {
        "id": "musicbrainz",
        "crate_name": "aether-plugin-musicbrainz",
        "dir": "Aether-Plugins-MusicBrainz",
        "tag_prefix": "musicbrainz"
    },
    {
        "id": "omdb",
        "crate_name": "aether-plugin-omdb",
        "dir": "Aether-Plugins-OMDb",
        "tag_prefix": "omdb"
    },
    {
        "id": "tmdb",
        "crate_name": "aether-plugin-tmdb",
        "dir": "Aether-Plugins-TMDb",
        "tag_prefix": "tmdb"
    }
]

def run_git(args, cwd=None):
    res = subprocess.run(["git"] + args, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
    return res.stdout.strip()

def get_current_cargo_version(plugin_dir, repo_root):
    cargo_path = os.path.join(repo_root, plugin_dir, "Cargo.toml")
    with open(cargo_path, "r", encoding="utf-8") as f:
        content = f.read()
    m = re.search(r'version\s*=\s*"([^"]+)"', content)
    if m:
        return m.group(1)
    return "0.1.0"

def get_latest_plugin_tag(tag_prefix, repo_root):
    try:
        tags = run_git(["tag", "-l", f"{tag_prefix}-v*"], cwd=repo_root).splitlines()
        if not tags:
            return None
        # Sort by semver if possible
        def sort_key(t):
            v_str = t.replace(f"{tag_prefix}-v", "")
            return [int(x) if x.isdigit() else 0 for x in v_str.split(".")]
        tags.sort(key=sort_key)
        return tags[-1]
    except Exception:
        return None

def parse_semver(v_str):
    parts = v_str.split(".")
    major = int(parts[0]) if len(parts) > 0 and parts[0].isdigit() else 0
    minor = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0
    patch = int(parts[2]) if len(parts) > 2 and parts[2].isdigit() else 0
    return (major, minor, patch)

def is_doc_only(changed_files):
    for f in changed_files:
        f_lower = f.lower()
        if not (f_lower.endswith(".md") or f_lower.endswith(".txt") or "/docs/" in f_lower or f_lower.startswith("docs/")):
            return False
    return True

def main():
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    
    # Options manuelles
    # Usage: python detect_changed_plugins.py [--manual-plugin <id>] [--manual-bump <MAJOR|MINOR>] [--base-ref <ref>]
    manual_plugin = None
    manual_bump = None
    base_ref = None

    args = sys.argv[1:]
    i = 0
    while i < len(args):
        if args[i] == "--manual-plugin" and i + 1 < len(args):
            manual_plugin = args[i+1]
            i += 2
        elif args[i] == "--manual-bump" and i + 1 < len(args):
            manual_bump = args[i+1].upper()
            i += 2
        elif args[i] == "--base-ref" and i + 1 < len(args):
            base_ref = args[i+1]
            i += 2
        else:
            i += 1

    # Déterminer la référence git de base pour la comparaison
    if not base_ref:
        # Si on est sur un merge HEAD, comparer HEAD~1..HEAD par défaut
        try:
            run_git(["rev-parse", "HEAD~1"], cwd=repo_root)
            base_ref = "HEAD~1"
        except Exception:
            base_ref = "HEAD"

    try:
        changed_paths = run_git(["diff", "--name-only", f"{base_ref}..HEAD"], cwd=repo_root).splitlines()
    except Exception:
        changed_paths = []

    # Vérifier si l'API commune a changé (hors docs)
    api_changed_files = [p for p in changed_paths if p.startswith("crates/aether-plugin-api/")]
    shared_api_changed = bool(api_changed_files) and not is_doc_only(api_changed_files)

    with open(os.path.join(repo_root, "repository", "manifest.json"), encoding="utf-8") as catalog_file:
        published_ids = {entry["manifest"]["id"] for entry in json.load(catalog_file)["extensions"]}

    results = []

    for p in PLUGINS_MAP:
        p_id = p["id"]
        p_dir = p["dir"]
        tag_prefix = p["tag_prefix"]
        crate_name = p["crate_name"]

        current_ver = get_current_cargo_version(p_dir, repo_root)
        major, minor, patch = parse_semver(current_ver)

        is_triggered = False
        bump_type = "MINOR"
        reason = ""

        if manual_plugin and (manual_plugin == p_id or manual_plugin == p_dir or manual_plugin == crate_name):
            is_triggered = True
            bump_type = manual_bump if manual_bump in ["MAJOR", "MINOR"] else "MAJOR"
            reason = f"Manual dispatch requested ({bump_type})"
        else:
            p_files = [path for path in changed_paths if path.startswith(p_dir + "/")]
            if p_files and not is_doc_only(p_files):
                is_triggered = True
                bump_type = "MINOR"
                reason = f"Source changes in {p_dir}"
            elif shared_api_changed:
                is_triggered = True
                bump_type = "MINOR"
                reason = "Shared aether-plugin-api changed"

        # Bootstrap source plugins missing from the signed catalogue on automatic runs.
        # A documentation-only push must also recover an unpublished first release.
        if not manual_plugin and p_id not in published_ids:
            is_triggered = True
            reason = "First signed catalogue release missing"

        if is_triggered:
            if bump_type == "MAJOR":
                next_ver = f"{major + 1}.0.0"
            else:
                next_ver = f"{major}.{minor + 1}.0"
            
            tag_name = f"{tag_prefix}-v{next_ver}"

            # Vérifier si ce tag exact existe déjà dans git
            tag_exists = False
            try:
                run_git(["rev-parse", tag_name], cwd=repo_root)
                tag_exists = True
            except Exception:
                tag_exists = False

            results.append({
                "id": p_id,
                "name": crate_name,
                "dir": p_dir,
                "tag_prefix": tag_prefix,
                "current_version": current_ver,
                "next_version": next_ver,
                "tag": tag_name,
                "bump_type": bump_type,
                "reason": reason,
                "tag_exists": tag_exists
            })

    output_json = json.dumps(results, indent=2)
    print(output_json)

if __name__ == "__main__":
    main()
