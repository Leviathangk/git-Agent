"""Stage pinned plugin release packages for installers; download through authenticated gh."""
import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def gh(*args):
    result = subprocess.run(["gh", *args], capture_output=True, text=True, encoding="utf-8",
                            creationflags=0x08000000 if __import__("os").name == "nt" else 0)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "gh failed; authenticate with gh auth login")
    return result.stdout


def verify_revision(record):
    value = json.loads(gh("api", f"repos/{record['repository']}/git/ref/tags/{record['tag']}"))["object"]
    for _ in range(5):
        if value["type"] == "commit":
            if value["sha"] != record["revision"]:
                raise ValueError("Plugin release tag differs from the pinned commit")
            return
        if value["type"] != "tag":
            break
        value = json.loads(gh("api", f"repos/{record['repository']}/git/tags/{value['sha']}"))["object"]
    raise ValueError("Cannot resolve plugin release commit")


def stage(target, output, source):
    lock = json.loads((ROOT / "bundled-plugins.lock.json").read_text(encoding="utf-8"))
    if lock["format"] != 1:
        raise ValueError("Unsupported plugin lock format")
    output.mkdir(parents=True, exist_ok=True)
    for record in lock["plugins"]:
        name = f"{record['id']}-{record['version']}-{target}.gaplugin"
        with tempfile.TemporaryDirectory(prefix="git-agent-plugin-download-") as temporary:
            directory = Path(temporary)
            if source:
                directory = source
            else:
                gh("auth", "status")
                verify_revision(record)
                gh("release", "download", record["tag"], "--repo", record["repository"],
                   "--pattern", name, "--pattern", name + ".sha256", "--dir", str(directory))
            package = directory / name
            expected = (directory / (name + ".sha256")).read_text(encoding="utf-8").split()
            if len(expected) != 2 or expected[1] != name or hashlib.sha256(package.read_bytes()).hexdigest() != expected[0]:
                raise ValueError("Plugin package SHA256 mismatch")
            with zipfile.ZipFile(package) as archive:
                info = json.loads(archive.read("package.json"))
                manifest = json.loads(archive.read("plugin.json"))
                if info["target"] != target or info["id"] != record["id"] or info["version"] != record["version"] or manifest["id"] != record["id"] or manifest["version"] != record["version"]:
                    raise ValueError("Plugin archive does not match the version lock")
            shutil.copyfile(package, output / name)
            print("Staged " + name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-packages", type=Path, help="Offline local verification; release CI omits this")
    args = parser.parse_args()
    stage(args.target, args.output.resolve(), args.source_packages.resolve() if args.source_packages else None)
