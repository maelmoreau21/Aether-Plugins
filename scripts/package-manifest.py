"""Project provider metadata onto the strict Core package manifest contract."""
import argparse
import hashlib
import json
from pathlib import Path

FIELDS = (
    "apiVersion", "id", "name", "version", "description", "author", "license",
    "repository", "permissions", "httpHosts", "category", "targetAbi",
    "minServerVersion", "maxServerVersion", "checksum", "banner", "changelog",
    "releaseDate", "configSchema",
)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("input","plugin-root","version","wasm","repository","output"):
        parser.add_argument("--"+name,required=True)
    args=parser.parse_args()
    source=json.loads(Path(args.input).read_text(encoding="utf-8"))
    manifest={key:source[key] for key in FIELDS if key in source}
    manifest.update(apiVersion=1,version=args.version,repository=args.repository,
                    checksum=hashlib.sha256(Path(args.wasm).read_bytes()).hexdigest())
    for key in ("id","name","description","author","license","permissions"):
        if not manifest.get(key):raise ValueError("Missing package manifest field: "+key)
    banner=manifest.get("banner")
    if banner and banner.startswith("assets/"):
        root=Path(args.plugin_root).resolve()
        asset=(root/banner).resolve()
        asset.relative_to(root)
        manifest["banner"]=asset.read_text(encoding="utf-8")
    Path(args.output).write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")

if __name__=="__main__":main()
