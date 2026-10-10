"""Keep recent runs of one workflow; deletion requires an explicit flag."""

import argparse
import json
import subprocess


def runs_to_delete(runs, keep, current_run_id):
    if keep < 1:
        raise ValueError("keep must be at least 1")
    ordered = sorted(
        {run["id"]: run for run in runs}.values(),
        key=lambda run: (run["created_at"], run["id"]),
        reverse=True,
    )
    return [
        run["id"]
        for run in ordered[keep:]
        if run["status"] == "completed" and run["id"] != current_run_id
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--workflow", required=True)
    parser.add_argument("--keep", type=int, default=3)
    parser.add_argument("--current-run-id", type=int, default=0)
    parser.add_argument("--delete", action="store_true")
    args = parser.parse_args()
    endpoint = f"/repos/{args.repository}/actions/workflows/{args.workflow}/runs?per_page=100"
    pages = json.loads(subprocess.check_output(
        ["gh", "api", "--paginate", "--slurp", endpoint], text=True, encoding="utf-8"
    ))
    runs = [run for page in pages for run in page["workflow_runs"]]
    selected = runs_to_delete(runs, args.keep, args.current_run_id)
    print(f"{args.workflow}: {len(runs)} runs, keep newest {args.keep}, "
          f"{len(selected)} older completed runs selected", flush=True)
    for run_id in selected:
        print(f"{'Delete' if args.delete else 'Would delete'} run {run_id}", flush=True)
        if args.delete:
            subprocess.run([
                "gh", "api", "--method", "DELETE",
                f"/repos/{args.repository}/actions/runs/{run_id}",
            ], check=True)


if __name__ == "__main__":
    main()
