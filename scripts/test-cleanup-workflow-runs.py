import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "cleanup", Path(__file__).with_name("cleanup-workflow-runs.py")
)
cleanup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cleanup)


def run(run_id, status="completed", created_at="2026-10-10T00:00:00Z"):
    return {"id": run_id, "status": status, "created_at": created_at}


class CleanupTests(unittest.TestCase):
    def test_newest_three_are_kept(self):
        self.assertEqual(cleanup.runs_to_delete([run(i) for i in range(1, 9)], 3, 8),
                         [5, 4, 3, 2, 1])

    def test_active_and_current_runs_are_protected(self):
        rows = [run(7), run(6), run(5), run(4, "in_progress"),
                run(3, "queued"), run(2, "waiting"), run(1)]
        self.assertEqual(cleanup.runs_to_delete(rows, 3, 1), [])

    def test_created_time_precedes_id(self):
        rows = [run(5), run(1, created_at="2026-10-11T00:00:00Z")]
        self.assertEqual(cleanup.runs_to_delete(rows, 1, 0), [5])

    def test_duplicate_pages_are_deduplicated(self):
        self.assertEqual(cleanup.runs_to_delete([run(3), run(3), run(2), run(1)], 2, 0), [1])

    def test_more_than_one_hundred_runs(self):
        self.assertEqual(len(cleanup.runs_to_delete([run(i) for i in range(1, 206)], 3, 0)), 202)

    def test_invalid_retention_fails_closed(self):
        with self.assertRaises(ValueError):
            cleanup.runs_to_delete([run(1)], 0, 0)

    def test_both_workflows_use_shared_cleanup(self):
        root = Path(__file__).resolve().parents[1]
        for name in ("build.yml", "website.yml"):
            source = (root / ".github" / "workflows" / name).read_text(encoding="utf-8")
            self.assertIn("actions: write", source)
            self.assertIn("scripts/cleanup-workflow-runs.py", source)
            self.assertIn(f"--workflow {name}", source)
            self.assertIn("--keep 3", source)
        website = (root / ".github/workflows/website.yml").read_text(encoding="utf-8")
        self.assertIn("github.event_name != 'pull_request'", website)


if __name__ == "__main__":
    unittest.main()
