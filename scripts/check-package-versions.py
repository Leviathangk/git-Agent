"""Check installer version fields before expensive release builds."""
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
BASH = shutil.which("bash")
if os.name == "nt":
    git_bash = Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe"
    if git_bash.exists():
        BASH = str(git_bash)
if not BASH:
    raise SystemExit("Bash is required to check installer version fields")


def version_fields(script, end, variable, tag):
    source = (ROOT / script).read_text(encoding="utf-8")
    prefix = 'version="${1#v}"'
    fragment = prefix + source.split(prefix, 1)[1].split(end, 1)[0]
    result = subprocess.run(
        [BASH, "-c", fragment + '\nprintf "%s" "$' + variable + '"', "version-check", tag],
        capture_output=True, text=True, encoding="utf-8", check=True,
    )
    return result.stdout


cases = [
    ("v2.0.0", "2.0.0"),
    ("v2.0.0-alpha.1", "2.0.0~alpha.1"),
    ("v2.0.0-beta.3", "2.0.0~beta.3"),
    ("v2.0.0-rc.1", "2.0.0~rc.1"),
    ("v2.0.0+build-hash", "2.0.0"),
    ("v2.0.0-beta.3+build-hash", "2.0.0~beta.3"),
]
for tag, expected in cases:
    deb = version_fields("installer/linux/package-deb.sh", "architecture=", "deb_version", tag)
    mac = version_fields("installer/macos/package.sh", "binary_dir=", "bundle_version", tag)
    assert deb == expected, (tag, deb, expected)
    assert mac == "2.0.0", (tag, mac)
    if shutil.which("dpkg"):
        subprocess.run(["dpkg", "--validate-version", deb], check=True)
        if "~" in expected:
            subprocess.run(["dpkg", "--compare-versions", deb, "lt", "2.0.0"], check=True)
    print(f"PASS {tag}: Debian={deb}, macOS={mac}")
