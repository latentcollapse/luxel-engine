import unittest

from pipeline.benchmark_mvp import compare


def _record(workflow: str) -> dict:
    return {
        "workflow": workflow,
        "task_id": "gate-run",
        "outcome": "pass",
        "wall_clock_seconds": 10.0 if workflow == "luxel" else 20.0,
        "successful_completion": True,
        "repair_iterations": 1 if workflow == "luxel" else 3,
        "induced_failure_recovered": True,
        "deterministic_rebuild": True,
        "deterministic_replay": True,
        "evidence_coverage": 1.0,
        "mechanical_correctness": 1.0,
        "visual_runtime_quality": 0.9,
        "manual_intervention_count": 0 if workflow == "luxel" else 2,
    }


class BenchmarkTests(unittest.TestCase):
    def test_comparison_preserves_missing_measurements(self) -> None:
        baseline = _record("conventional_engine_mcp")
        baseline["visual_runtime_quality"] = None
        report = compare(_record("luxel"), baseline)
        self.assertIsNone(report["deltas_luxel_minus_baseline"]["visual_runtime_quality"])
        self.assertIn("visual_runtime_quality", report["missing_measurements"])

    def test_mismatched_tasks_are_rejected(self) -> None:
        baseline = _record("conventional_engine_mcp")
        baseline["task_id"] = "other"
        with self.assertRaisesRegex(ValueError, "same task_id"):
            compare(_record("luxel"), baseline)


if __name__ == "__main__":
    unittest.main()
