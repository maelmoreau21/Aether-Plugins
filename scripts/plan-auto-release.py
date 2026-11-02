"""Plan numeric signed package versions; beta status belongs to the GitHub release."""
import importlib.util,json,subprocess,sys
sys.dont_write_bytecode=True
from pathlib import Path

def next_version(source,published):
    source=tuple(map(int,source.split(".")))
    old=tuple(map(int,published.split("."))) if published else (0,0,0)
    return ".".join(map(str,source if source>old else (*old[:2],old[2]+1)))

def main():
    spec=importlib.util.spec_from_file_location("detect",Path(__file__).with_name("detect_changed_plugins.py"))
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    root=Path(__file__).resolve().parent.parent
    base=subprocess.check_output(["git","log","-1","--format=%H","--","repository/manifest.json"],text=True).strip()
    changed=subprocess.check_output(["git","diff","--name-only",base+"..HEAD"],text=True).splitlines()
    shared=any(p in ("Cargo.toml","Cargo.lock") or p.startswith("crates/aether-plugin-api/") for p in changed if not p.endswith(".md"))
    catalog=json.loads((root/"repository/manifest.json").read_text())
    published={e["manifest"]["id"]:e["manifest"]["version"] for e in catalog["extensions"]}
    plan=[]
    for p in module.PLUGINS_MAP:
        version=module.get_current_cargo_version(p["dir"],str(root))
        files=[f for f in changed if f.startswith(p["dir"]+"/")]
        if not (shared or (files and not module.is_doc_only(files)) or p["id"] not in published or tuple(map(int,version.split(".")))>tuple(map(int,published[p["id"]].split(".")))):continue
        v=next_version(version,published.get(p["id"]))
        if (root/"repository/packages"/f"{p['tag_prefix']}-{v}.package.json").exists():raise RuntimeError("Signed package already exists: "+p["id"]+"@"+v)
        plan.append({**p,"name":p["crate_name"],"next_version":v})
    print(json.dumps(plan))
if __name__=="__main__":main()
