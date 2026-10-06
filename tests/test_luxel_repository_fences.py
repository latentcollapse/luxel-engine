from __future__ import annotations

import unittest
from pathlib import Path

from pipeline.luxel_repository_fences import violations


LUXEL_ROOT = Path(__file__).resolve().parents[1]


class RepositoryFenceTests(unittest.TestCase):
    def test_native_sources_have_no_archived_import_or_path_dependency(self) -> None:
        self.assertEqual([], violations(LUXEL_ROOT))


if __name__ == "__main__":
    unittest.main()
