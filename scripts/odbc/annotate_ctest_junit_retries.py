#!/usr/bin/env python3
"""Inject Jenkins flakyFailure / rerunFailure into CTest JUnit.

CTest ``--repeat until-pass:N --output-junit`` keeps only the last attempt.
The verbose log still lists every Failed/Passed line, so we reconstruct retry
history and write Surefire-style children the dashboard parser already reads.

CTest also writes ``<failure message="Failed"/>`` with no body and keeps the
test output only in ``<system-out>``. The Catch2 failure section of that
output is copied into the ``<failure>`` text, and its first assertion line
replaces the generic message.
"""

from __future__ import annotations

import argparse
import re
import sys
import xml.etree.ElementTree as ET
from collections import defaultdict
from pathlib import Path

TEST_LINE = re.compile(
    r"Test\s+#\d+:\s+(.+?)\s+\.{3,}\s*(?:\*\*\*)?(Passed|Failed|Timeout|Exception|Not Run)",
    re.IGNORECASE,
)

FAILED = {"failed", "timeout", "exception"}

CATCH2_FAILED_LINE = re.compile(r"^.*:\d+: FAILED:.*$", re.MULTILINE)
FAILURE_TEXT_LIMIT = 8000
FAILURE_MESSAGE_LIMIT = 500


def parse_attempts(log_text: str) -> dict[str, list[str]]:
    attempts: dict[str, list[str]] = defaultdict(list)
    for match in TEST_LINE.finditer(log_text):
        name, outcome = match.group(1).rstrip(), match.group(2).lower()
        attempts[name].append("failed" if outcome in FAILED else outcome.replace(" ", "").lower())
    return dict(attempts)


def _testcase_name(case: ET.Element) -> str:
    name = case.get("name") or ""
    classname = case.get("classname") or ""
    if classname and name.startswith(classname):
        return name
    if classname and name:
        return f"{classname}: {name}"
    return name or classname


def _failure_details(output: str) -> tuple[str, str | None]:
    match = CATCH2_FAILED_LINE.search(output)
    if match is None:
        return output[-FAILURE_TEXT_LIMIT:], None
    text = output[match.start() :][:FAILURE_TEXT_LIMIT]
    summary = match.group(0).strip()
    for line in text.splitlines()[1:]:
        if line[:1].isspace() and line.strip():
            summary = f"{summary} {line.strip()}"
            break
    return text, summary[:FAILURE_MESSAGE_LIMIT]


def _attach_failure_output(case: ET.Element) -> None:
    failure = case.find("failure")
    if failure is None or (failure.text or "").strip():
        return
    output = (case.findtext("system-out") or "").strip()
    if not output:
        return
    text, summary = _failure_details(output)
    failure.text = text
    if summary and failure.get("message", "Failed") == "Failed":
        failure.set("message", summary)


def annotate(junit_xml: str, attempts: dict[str, list[str]]) -> str:
    root = ET.fromstring(junit_xml)
    suites = root.findall(".//testcase")
    if root.tag == "testcase":
        suites = [root]
    for case in suites:
        _attach_failure_output(case)
        key = _testcase_name(case)
        history = attempts.get(key) or attempts.get(case.get("name") or "")
        if not history or len(history) < 2:
            continue
        last = history[-1]
        prior = history[:-1]
        failed_prior = [h for h in prior if h == "failed"]
        if not failed_prior:
            continue
        if last == "passed":
            for i, _ in enumerate(failed_prior, start=1):
                child = ET.Element(
                    "flakyFailure",
                    {"type": "CTestFailure", "message": f"ctest attempt {i} failed"},
                )
                case.append(child)
        elif last == "failed":
            for i, _ in enumerate(failed_prior, start=1):
                child = ET.Element(
                    "rerunFailure",
                    {"type": "CTestFailure", "message": f"ctest attempt {i} failed"},
                )
                case.append(child)
    return ET.tostring(root, encoding="unicode")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--junit", required=True, type=Path)
    parser.add_argument("--log", required=True, type=Path)
    args = parser.parse_args(argv)
    if not args.junit.is_file() or not args.log.is_file():
        print(f"annotate_ctest_junit_retries: missing {args.junit} or {args.log}", file=sys.stderr)
        return 0
    xml_text = args.junit.read_text(encoding="utf-8", errors="replace")
    log_text = args.log.read_text(encoding="utf-8", errors="replace")
    updated = annotate(xml_text, parse_attempts(log_text))
    if not updated.startswith("<?xml"):
        updated = '<?xml version="1.0" encoding="UTF-8"?>\n' + updated
    args.junit.write_text(updated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
