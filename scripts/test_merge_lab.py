"""Prepare disposable test-repo worktrees and retain reproducible merge evidence."""
import argparse
import hashlib
import json
import subprocess
from datetime import datetime
from pathlib import Path

SOURCE = Path("D:/workspace/test-repo")
PARENT = Path("D:/workspace/test-repo-tests")


def command(args, cwd=None, accepted=(0,)):
    result = subprocess.run(args, cwd=cwd, capture_output=True,
                            creationflags=0x08000000, encoding="utf-8", errors="replace")
    if result.returncode not in accepted:
        raise RuntimeError(f"{args}: {result.stdout}\n{result.stderr}")
    return result.stdout


def git(root, *args, accepted=(0,)):
    return command(["git", "-C", str(root), *args], accepted=accepted)


def snapshot():
    return {
        "head": git(SOURCE, "rev-parse", "HEAD").strip(),
        "merge_head": git(SOURCE, "rev-parse", "MERGE_HEAD").strip(),
        "index": git(SOURCE, "ls-files", "--stage", "-z"),
        "files": {str(p.relative_to(SOURCE)).replace("\\", "/"):
                  hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in SOURCE.rglob("*") if p.is_file() and ".git" not in p.relative_to(SOURCE).parts},
    }


def save(path, data):
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def prepare():
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    root = PARENT / stamp
    root.mkdir(parents=True, exist_ok=False)
    save(root / "original-snapshot.json", snapshot())
    master = root / "repository"
    command(["git", "clone", "--no-local", "--origin", "fixture-source", "--no-checkout", str(SOURCE), str(master)])
    git(master, "config", "user.name", "Git Agent Merge Tester")
    git(master, "config", "user.email", "merge-test@example.test")
    git(master, "config", "core.autocrlf", "false")
    git(master, "remote", "add", "origin", "git@github.com:adoin/test-repo.git")
    prefix = f"codex/merge-validation/{stamp}"
    folder = "merge-lab/validation-" + stamp
    refs = {}

    def branch(name, parent, files):
        ref = prefix + "/" + name
        git(master, "checkout", "-b", ref, parent)
        for path, text in files.items():
            target = master / path
            if text is None:
                target.unlink()
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(text, encoding="utf-8")
        git(master, "add", "--", folder)
        git(master, "commit", "-m", "test: " + name)
        refs[name] = ref
        return ref

    base = branch("base", "fixture-source/main", {
        folder + "/story.txt": "base\n",
        folder + "/重命名.txt": "one\ntwo\nthree\nfour\nfive\n",
        folder + "/retired.mjs": "export const version = 'base';\n",
    })
    branch("clean-right", base, {folder + "/right.txt": "right\n"})
    branch("clean-left", base, {folder + "/left.txt": "left\n"})
    branch("conflict-left", base, {folder + "/story.txt": "left\n", folder + "/retired.mjs": None, folder + "/added.mjs": "export const features = ['left'];\n"})
    branch("conflict-right", base, {folder + "/story.txt": "right\n", folder + "/retired.mjs": "export const version = 'updated';\n", folder + "/added.mjs": "export const features = ['right'];\n"})
    branch("rename-left", base, {folder + "/重命名.txt": None, folder + "/renamed.txt": "one\ntwo\nthree\nfour\nfive\n"})
    branch("rename-right", base, {folder + "/重命名.txt": "one\ntwo\nTHREE\nfour\nfive\n"})

    cases = {}
    for name, start in [("ff", "base"), ("no-ff", "base"), ("no-commit", "clean-left"),
                        ("clean", "clean-left"), ("rename", "rename-left"),
                        ("conflict", "conflict-left"), ("rebase-abort", "conflict-left"),
                        ("rebase-continue", "conflict-left"), ("cherry-pick", "conflict-left")]:
        directory = root / name
        git(master, "worktree", "add", "-b", prefix + "/local-" + name, str(directory), refs[start])
        cases[name] = str(directory)

    complex_repo = root / "complex-ai"
    git(master, "branch", prefix + "/complex-right", "fixture-source/codex/complex-checkout-right")
    git(master, "worktree", "add", "-b", prefix + "/complex-ai", str(complex_repo), "fixture-source/codex/complex-checkout-left")
    git(complex_repo, "merge", "--no-commit", prefix + "/complex-right", accepted=(1,))
    # Match the existing Middle draft as well as its committed stages.
    for file in (SOURCE / "merge-lab/complex-checkout").glob("*"):
        if file.is_file():
            target = complex_repo / "merge-lab/complex-checkout" / file.name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(file.read_bytes())

    expected = root / "expert-baseline"
    expected.mkdir()
    source_dir = SOURCE / "merge-lab/complex-checkout"
    def resolve(path, replacements):
        text = (source_dir / path).read_text(encoding="utf-8")
        import re
        blocks = list(re.finditer(r"<<<<<<< HEAD\n(.*?)=======\n(.*?)>>>>>>>[^\n]*\n", text, re.S))
        assert len(blocks) == len(replacements), (path, len(blocks))
        for match, replacement in reversed(list(zip(blocks, replacements))):
            text = text[:match.start()] + replacement + text[match.end():]
        (expected / path).write_text(text, encoding="utf-8")

    resolve("customer.mjs", [
        "  return value.normalize('NFKC').trim().replace(/\\s+/g, ' ').toLowerCase();\n",
        "  return `customer:${normalizeCustomer(value).replace(/\\s/g, '-')}`;\n",
        "  const customer = normalizeCustomer(value);\n  const [name, domain] = customer.split('@');\n  const masked = domain ? `${name.slice(0, 1)}***@${domain}` : customer;\n  return `${masked} (${locale})`;\n",
    ])
    resolve("pricing.mjs", [
        "  const member = input.tier === 'gold' ? roundMoney(subtotal * 0.1) : 0;\n  const coupon = input.coupon === 'WELCOME5' ? 5 : 0;\n  return Math.min(subtotal, roundMoney(member + coupon));\n",
        "  if (subtotal >= 80) return 0;\n  return service === 'express' ? 15 : 7;\n",
        "  return roundMoney(amount * (region === 'EU' ? 0.2 : region === 'US' ? 0.08 : 0));\n",
    ])
    resolve("order.mjs", [
        "import { capturePayment } from './payments.mjs';\nimport { recordPaymentAudit } from './audit.mjs';\n\nexport const EXPECTED_STEP_COUNT = 7;\n",
        "  return ['validate-cart', 'screen-risk', 'reserve-stock', 'calculate-tax', 'capture-payment', 'persist-audit', 'send-receipt'];\n",
        "  return { orderId: input.id, customer: normalizeCustomer(input.customer), customerId: customerKey(input.customer), currency: input.currency ?? 'USD', locale: input.locale ?? 'en', traceId: `trace:${input.id}` };\n",
        "  return input.flagged !== true && (input.availableStock ?? Infinity) >= itemCount(input);\n",
        "  return { orderId: input.id, amountMinor: Math.round(quote.total * 100), currency: context.currency, idempotencyKey: `${context.customerId}:${input.id}`, simulatedFailures: input.simulatedFailures ?? 0, traceId: context.traceId };\n",
        "export function formatReceipt(input, context, quote, payment, audit) {\n  return `Order ${input.id} | ${receiptCustomer(input.customer, context.locale)} | ${formatAmount(quote.total, context.currency)} | Audit ${audit.id}`;\n",
        "  const payment = capturePayment(paymentRequest(input, context, quote));\n  const audit = recordPaymentAudit(context, quote, payment);\n  return { context, quote, payment, audit, receipt: formatReceipt(input, context, quote, payment, audit) };\n",
    ])
    payment = (source_dir / "payments.mjs").read_text(encoding="utf-8")
    payment = payment.replace("  return {", "  const failures = request.simulatedFailures ?? 0;\n  if (failures >= 3) throw new Error('Payment unavailable after 3 attempts');\n  return {")
    payment = payment.replace("attempts: 1", "attempts: failures + 1")
    (expected / "payments.mjs").write_text(payment, encoding="utf-8")
    save(root / "baseline-matrix.json", {
        "customer.mjs": "Manual: NFKC + collapse whitespace + lowercase; hyphenated key; masked email + locale",
        "pricing.mjs": "Manual: stack member/coupon with cap; free shipping >=80 else 7/15; EU20% and US8%",
        "order.mjs": "Manual: seven ordered stages; all context fields; risk AND stock; integer minor units + retry input; audit + private receipt",
        "legacy-payment.mjs": "Left deletion: obsolete adapter stays deleted; migrate retry behavior to payments.mjs",
        "payments.mjs": "Sibling edit required: migrate max three attempts, independent of textual conflicts",
    })
    manifest = {"source": str(SOURCE), "root": str(root), "master": str(master),
                "prefix": prefix, "folder": folder, "refs": refs, "cases": cases,
                "complex": str(complex_repo), "expected": str(expected)}
    save(root / "manifest.json", manifest)
    # Independently execute the expert result before contacting any model.
    baseline_repo = root / "baseline-result"
    git(master, "worktree", "add", "-b", prefix + "/baseline", str(baseline_repo), "fixture-source/codex/complex-checkout-left")
    git(baseline_repo, "merge", "--no-commit", prefix + "/complex-right", accepted=(1,))
    destination = baseline_repo / "merge-lab/complex-checkout"
    for file in expected.glob("*.mjs"):
        (destination / file.name).write_bytes(file.read_bytes())
    (destination / "legacy-payment.mjs").unlink()
    tests = command(["node", "--test", *[str(f) for f in destination.glob("*.test.mjs")]], cwd=baseline_repo)
    acceptance = command(["node", str(destination / "acceptance.mjs")], cwd=baseline_repo)
    (root / "baseline-tests.txt").write_text(tests + acceptance, encoding="utf-8")
    print(json.dumps(manifest, ensure_ascii=False))


def verify(root):
    before = json.loads((root / "original-snapshot.json").read_text(encoding="utf-8"))
    if before != snapshot():
        raise RuntimeError("Original test-repo changed during the test; inspect snapshot before continuing")
    print("Original test-repo HEAD, MERGE_HEAD, index stages and worktree are unchanged")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["prepare", "verify"])
    parser.add_argument("root", nargs="?")
    args = parser.parse_args()
    prepare() if args.action == "prepare" else verify(Path(args.root))
