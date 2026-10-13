"""Reproduce a deleted component reference in an isolated clone of test-repo."""
import hashlib
import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path

SOURCE = Path("D:/workspace/test-repo")

def git(root, *args, accepted=(0,)):
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True,
                            encoding="utf-8", errors="replace", creationflags=0x08000000)
    if result.returncode not in accepted:
        raise RuntimeError(result.stderr)
    return result.stdout

def snapshot():
    return {
        "head": git(SOURCE, "rev-parse", "HEAD").strip(),
        "index": git(SOURCE, "ls-files", "--stage", "-z"),
        "status": git(SOURCE, "status", "--porcelain=v1", "-z"),
        "files": {str(path.relative_to(SOURCE)): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in SOURCE.rglob("*") if path.is_file()
                  and ".git" not in path.relative_to(SOURCE).parts},
    }

def main():
    before = snapshot()
    evidence = Path("D:/workspace/test-repo-tests") / ("deleted-component-" + datetime.now().strftime("%Y%m%d-%H%M%S"))
    evidence.mkdir(parents=True)
    repo = evidence / "repository"
    subprocess.run(["git", "-c", "core.autocrlf=false", "clone", "--no-local", str(SOURCE), str(repo)], check=True,
                   capture_output=True, creationflags=0x08000000)
    git(repo, "config", "user.name", "Merge Regression")
    git(repo, "config", "user.email", "merge-regression@example.test")
    git(repo, "config", "core.autocrlf", "false")
    folder = repo / "merge-lab/deleted-component"
    folder.mkdir(parents=True)
    consumer = folder / "Screen.tsx"
    component = folder / "RetiredPanel.tsx"
    component.write_text("export default function RetiredPanel() { return null; }\n", encoding="utf-8")
    consumer.write_text('import RetiredPanel from "./RetiredPanel";\nexport const screen = () => <RetiredPanel mode="base" />;\n', encoding="utf-8")
    git(repo, "add", "merge-lab/deleted-component")
    git(repo, "commit", "-m", "test: base component and consumer")
    base = git(repo, "rev-parse", "HEAD").strip()
    git(repo, "checkout", "-b", "codex/deleted-component-left")
    component.unlink()
    consumer.write_text('export const screen = () => null;\n', encoding="utf-8")
    git(repo, "add", "merge-lab/deleted-component")
    git(repo, "commit", "-m", "test: retire component and render path")
    git(repo, "checkout", "-b", "codex/deleted-component-right", base)
    consumer.write_text('import RetiredPanel from "./RetiredPanel";\nexport const screen = () => <RetiredPanel mode="audited" />;\n', encoding="utf-8")
    git(repo, "add", "merge-lab/deleted-component")
    git(repo, "commit", "-m", "test: update old render mode without knowing deletion")
    git(repo, "checkout", "codex/deleted-component-left")
    git(repo, "merge", "--no-commit", "codex/deleted-component-right", accepted=(1,))
    path = "merge-lab/deleted-component/Screen.tsx"
    stages = {name: git(repo, "show", f":{stage}:{path}")
              for name, stage in [("base", 1), ("left", 2), ("right", 3)]}
    expected = {
        "import": "Accept deletion; RetiredPanel.tsx is deleted in the merged index.",
        "render": "Left deletion or Manual preserving independently justified behavior; never retain a render/import of the deleted module.",
        "evidence": git(repo, "diff", "--cached", "--name-status", base),
    }
    assert before == snapshot(), "Original test-repo changed"
    manifest = {"repository": str(repo), "file": path, "base": base, "stages": stages,
                "baseline": expected, "original_snapshot": before}
    (evidence / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    print(str(evidence / "manifest.json"))

if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--verify":
        evidence = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
        assert evidence["original_snapshot"] == snapshot(), "Original test-repo changed"
        print("PASS: original test-repo HEAD, index, status and file contents unchanged")
    else:
        main()
