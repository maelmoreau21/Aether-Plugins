#!/usr/bin/env python3
"""
detect_changed_plugins.py
Detects modified plugins in Aether-Plugins between a base commit/tag and HEAD.

Rules:
- Each plugin has an independent lifecycle and versioning.
- Analyzes diff paths (`git diff --name-only <base>..HEAD`).
- Filters out documentation-only changes (.md, .txt, docs/).
- Detects shared API changes (`crates/aether-plugin-api/`).
- Channel awareness:
  - --channel develop : calculates next beta version (e.g. tmdb-v0.2.0-beta.1).
  - --channel main : calculates next stable version (e.g. tmdb-v0.2.0).
- Verifies package immutability: fails if attempting to modify an existing signed package.
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
        "id": "poster-rotator",
        "crate_name": "aether-plugin-poster-rotator",
        "dir": "Aether-Plugins-PosterRotator",
        "tag_prefix": "poster-rotator"
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
    try:
        with open(cargo_path, "r", encoding="utf-8") as f:
            content = f.read()
        m = re.search(r'version\s*=\s*"([^"]+)"', content)
        if m:
            return m.group(1)
    except Exception:
        pass
    return "0.1.0"

def get_all_plugin_tags(tag_prefix, repo_root):
    try:
        tags = run_git(["tag", "-l", f"{tag_prefix}-v*"], cwd=repo_root).splitlines()
        return [t.strip() for t in tags if t.strip()]
    except Exception:
        return []

def get_tags_at_head(repo_root):
    try:
        tags = run_git(["tag", "--points-at", "HEAD"], cwd=repo_root).splitlines()
        return [t.strip() for t in tags if t.strip()]
    except Exception:
        return []

def parse_semver(v_str):
    cleaned = v_str.split("-")[0]
    parts = cleaned.split(".")
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
    
    manual_plugin = None
    manual_bump = None
    base_ref = None
    channel = "main"

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
        elif args[i] == "--channel" and i + 1 < len(args):
            channel = args[i+1]
            i += 2
        else:
            i += 1

    if not base_ref:
        try:
            run_git(["rev-parse", "HEAD~1"], cwd=repo_root)
            base_ref = "HEAD~1"
        except Exception:
            base_ref = "HEAD"

    try:
        changed_paths = run_git(["diff", "--name-only", f"{base_ref}..HEAD"], cwd=repo_root).splitlines()
    except Exception:
        changed_paths = []

    # Check if common aether-plugin-api changed
    api_changed_files = [p for p in changed_paths if p.startswith("crates/aether-plugin-api/")]
    shared_api_changed = bool(api_changed_files) and not is_doc_only(api_changed_files)

    catalog_path = os.path.join(repo_root, "repository", "manifest.json")
    published_ids = set()
    if os.path.exists(catalog_path):
        with open(catalog_path, encoding="utf-8") as catalog_file:
            published_ids = {entry["manifest"]["id"] for entry in json.load(catalog_file).get("extensions", [])}

    head_tags = get_tags_at_head(repo_root)
    results = []

    for p in PLUGINS_MAP:
        p_id = p["id"]
        p_dir = p["dir"]
        tag_prefix = p["tag_prefix"]
        crate_name = p["crate_name"]

        current_ver = get_current_cargo_version(p_dir, repo_root)
        major, minor, patch = parse_semver(current_ver)

        is_triggered = False
        bump_type = "PATCH" if channel == "main" else "MINOR"
        reason = ""

        if manual_plugin and (manual_plugin == p_id or manual_plugin == p_dir or manual_plugin == crate_name):
            is_triggered = True
            bump_type = manual_bump if manual_bump in ["MAJOR", "MINOR", "PATCH"] else "PATCH"
            reason = f"Manual dispatch requested ({bump_type})"
        elif not manual_plugin:
            p_files = [path for path in changed_paths if path.startswith(p_dir + "/")]
            if p_files and not is_doc_only(p_files):
                is_triggered = True
                bump_type = "MINOR"
                reason = f"Source changes in {p_dir}"
            elif shared_api_changed:
                is_triggered = True
                bump_type = "MINOR"
                reason = "Shared aether-plugin-api changed"

        # Bootstrap plugins missing from published catalog on automatic runs
        if not manual_plugin and not is_triggered and p_id not in published_ids:
            is_triggered = True
            reason = "First signed catalogue release missing"
            bump_type = "PATCH" if current_ver != "0.1.0" else "NONE"

        if is_triggered:
            all_tags = get_all_plugin_tags(tag_prefix, repo_root)

            if channel == "develop":
                # Determine beta version: e.g. 0.2.0-beta.N
                cycle_target = f"{major}.{minor + 1}.0" if bump_type == "MINOR" else (f"{major + 1}.0.0" if bump_type == "MAJOR" else f"{major}.{minor}.{patch + 1}")
                beta_re = re.compile(rf"^{tag_prefix}-v{re.escape(cycle_target)}-beta\.(\d+)$")
                existing_betas = []
                for t in all_tags:
                    m = beta_re.match(t)
                    if m:
                        existing_betas.append(int(m.group(1)))
                existing_betas.sort()
                
                next_beta_idx = 0 if not existing_betas else (existing_betas[-1] + 1)
                next_ver = f"{cycle_target}-beta.{next_beta_idx}"
                tag_name = f"{tag_prefix}-v{next_ver}"

                # Idempotency check: is HEAD already tagged?
                tag_exists = any(t.startswith(f"{tag_prefix}-v{cycle_target}-beta.") for t in head_tags)
            else:
                # Channel main: stable version
                if bump_type == "MAJOR":
                    next_ver = f"{major + 1}.0.0"
                elif bump_type == "MINOR":
                    next_ver = f"{major}.{minor + 1}.0"
                elif bump_type == "PATCH":
                    next_ver = f"{major}.{minor}.{patch + 1}"
                else:
                    next_ver = current_ver
                
                tag_name = f"{tag_prefix}-v{next_ver}"
                tag_exists = tag_name in all_tags

            # Package immutability check
            pkg_file = os.path.join(repo_root, "repository", "packages", f"{tag_prefix}-{next_ver}.package.json")
            package_already_exists = os.path.exists(pkg_file)

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
                "tag_exists": tag_exists,
                "package_already_exists": package_already_exists,
                "channel": channel
            })

    output_json = json.dumps(results, indent=2)
    print(output_json)

if __name__ == "__main__":
    main()
