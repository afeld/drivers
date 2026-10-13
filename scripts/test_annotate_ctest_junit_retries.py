"""Unit tests for scripts/odbc/annotate_ctest_junit_retries.py.

Run with:
    python -m unittest scripts/test_annotate_ctest_junit_retries.py -v
"""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "odbc" / "annotate_ctest_junit_retries.py"


def _load():
    spec = importlib.util.spec_from_file_location("annotate_ctest_junit_retries", MODULE_PATH)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


class AnnotateCtestJunitRetriesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.mod = _load()

    def test_rescued_test_gets_flaky_failure_per_failed_attempt(self) -> None:
        log = """
        1/1 Test #1: connect: should login .....................***Failed  0.10 sec
        1/1 Test #1: connect: should login .....................***Failed  0.11 sec
        1/1 Test #1: connect: should login .....................   Passed  0.09 sec
        """
        junit = """
        <testsuite name="CTest">
          <testcase name="connect: should login" classname="connect" time="0.09"/>
        </testsuite>
        """
        xml = self.mod.annotate(junit, self.mod.parse_attempts(log))
        self.assertEqual(xml.count("flakyFailure"), 2)
        self.assertNotIn("rerunFailure", xml)
        self.assertIn("ctest attempt 1 failed", xml)
        self.assertIn("ctest attempt 2 failed", xml)

    def test_exhausted_test_gets_rerun_failure(self) -> None:
        log = """
        1/1 Test #4: foo: bar .....................***Failed  0.10 sec
        1/1 Test #4: foo: bar .....................***Failed  0.10 sec
        """
        junit = """
        <testsuite>
          <testcase name="foo: bar" classname="foo" time="0.10">
            <failure message="still red"/>
          </testcase>
        </testsuite>
        """
        xml = self.mod.annotate(junit, self.mod.parse_attempts(log))
        self.assertIn("rerunFailure", xml)
        self.assertNotIn("flakyFailure", xml)
        self.assertIn("<failure", xml)

    def test_first_pass_is_unchanged(self) -> None:
        log = "1/1 Test #2: stable: ok .....................   Passed  0.01 sec\n"
        junit = '<testsuite><testcase name="stable: ok" classname="stable" time="0.01"/></testsuite>'
        xml = self.mod.annotate(junit, self.mod.parse_attempts(log))
        self.assertNotIn("flakyFailure", xml)
        self.assertNotIn("rerunFailure", xml)

    def test_failure_gets_catch2_failure_section_from_system_out(self) -> None:
        junit = """<testsuite>
  <testcase name="t: c" classname="t: c" time="1" status="fail">
    <failure message="Failed" />
    <system-out>Filters: "c"
../tests/t.cpp:10: warning:
  retrying

../tests/t.cpp:20: FAILED:
  REQUIRE( ret == SQL_SUCCESS )
with expansion:
  -1 == 0
</system-out>
  </testcase>
</testsuite>"""
        root = ET.fromstring(self.mod.annotate(junit, {}))
        failure = root.find(".//failure")
        assert failure is not None
        self.assertEqual(
            failure.get("message"),
            "../tests/t.cpp:20: FAILED: REQUIRE( ret == SQL_SUCCESS )",
        )
        self.assertTrue(failure.text.startswith("../tests/t.cpp:20: FAILED:"))
        self.assertIn("-1 == 0", failure.text)
        self.assertNotIn("retrying", failure.text)
        self.assertIn("retrying", root.findtext(".//system-out"))

    def test_failure_without_catch2_marker_gets_truncated_output_tail(self) -> None:
        output = "x" * (self.mod.FAILURE_TEXT_LIMIT + 100) + "segfault"
        junit = (
            '<testsuite><testcase name="a" classname="a" status="fail">'
            f'<failure message="Failed"/><system-out>{output}</system-out>'
            "</testcase></testsuite>"
        )
        failure = ET.fromstring(self.mod.annotate(junit, {})).find(".//failure")
        assert failure is not None
        self.assertEqual(failure.get("message"), "Failed")
        self.assertEqual(len(failure.text), self.mod.FAILURE_TEXT_LIMIT)
        self.assertTrue(failure.text.endswith("segfault"))

    def test_failure_with_existing_text_is_unchanged(self) -> None:
        junit = (
            '<testsuite><testcase name="a" classname="a">'
            '<failure message="Timeout">kept</failure><system-out>../t.cpp:1: FAILED:</system-out>'
            "</testcase></testsuite>"
        )
        failure = ET.fromstring(self.mod.annotate(junit, {})).find(".//failure")
        assert failure is not None
        self.assertEqual(failure.text, "kept")
        self.assertEqual(failure.get("message"), "Timeout")

    def test_cli_rewrites_file(self) -> None:
        log = "1/1 Test #1: a: b .....................***Failed  0.1 sec\n1/1 Test #1: a: b .....................   Passed  0.1 sec\n"
        junit = '<testsuite><testcase name="a: b" classname="a" time="0.1"/></testsuite>'
        with tempfile.TemporaryDirectory() as tmp:
            jpath = Path(tmp) / "odbc-junit.xml"
            lpath = Path(tmp) / "odbc-junit.ctest.log"
            jpath.write_text(junit, encoding="utf-8")
            lpath.write_text(log, encoding="utf-8")
            rc = self.mod.main(["--junit", str(jpath), "--log", str(lpath)])
            self.assertEqual(rc, 0)
            body = jpath.read_text(encoding="utf-8")
            self.assertIn("flakyFailure", body)


if __name__ == "__main__":
    unittest.main()
