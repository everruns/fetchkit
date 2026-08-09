"""Regression tests for scripts/check_okf.py."""

import pathlib
import subprocess
import sys
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts" / "check_okf.py"

CONCEPT = """\
---
type: Subsystem Design
title: Widget
description: One sentence about the widget.
---

# Widget

See [Gadget](gadget.md).
"""

GADGET = """\
---
type: Subsystem Design
title: Gadget
description: One sentence about the gadget.
---

# Gadget

See [Widget](widget.md).
"""

ROOT_INDEX = """\
---
okf_version: "0.2"
---

# Bundle

* [Widget](widget.md) - One sentence about the widget.
* [Gadget](gadget.md) - One sentence about the gadget.
"""

LOG = """\
# Bundle Update Log

## 2026-08-08

* **Creation**: Added [Widget](widget.md).
"""


def run(bundle: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), str(bundle)],
        capture_output=True,
        text=True,
        check=False,
    )


class CheckOkfTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temp = tempfile.TemporaryDirectory()
        self.addCleanup(self._temp.cleanup)
        self.bundle = pathlib.Path(self._temp.name) / "bundle"
        self.bundle.mkdir()
        (self.bundle / "index.md").write_text(ROOT_INDEX)
        (self.bundle / "log.md").write_text(LOG)
        (self.bundle / "widget.md").write_text(CONCEPT)
        (self.bundle / "gadget.md").write_text(GADGET)

    def test_conformant_bundle_is_accepted(self) -> None:
        result = run(self.bundle)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OKF v0.2 conformant", result.stdout)

    def test_missing_frontmatter_is_rejected(self) -> None:
        (self.bundle / "widget.md").write_text("# Widget\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing YAML frontmatter", result.stderr)

    def test_missing_type_is_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT.replace("type: Subsystem Design\n", "")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("non-empty 'type'", result.stderr)

    def test_unlisted_concept_is_rejected(self) -> None:
        (self.bundle / "orphan.md").write_text(CONCEPT)
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("orphan.md: not listed", result.stderr)

    def test_dangling_link_is_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT.replace("gadget.md", "missing.md")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("link target does not exist", result.stderr)

    def test_disconnected_concept_is_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(CONCEPT.replace("See [Gadget](gadget.md).", ""))
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("links to no other concept", result.stderr)

    def test_repository_path_reference_is_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT + "\nRead `knowledge/gadget.md`.\n"
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("not as repository paths", result.stderr)

    def test_bad_log_heading_is_rejected(self) -> None:
        (self.bundle / "log.md").write_text("# Log\n\n## August 2026\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("is not '## YYYY-MM-DD'", result.stderr)


if __name__ == "__main__":
    unittest.main()
