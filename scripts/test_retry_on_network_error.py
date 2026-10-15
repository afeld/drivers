"""Unit tests for scripts/retry_on_network_error.sh.

Run with:
    python3 -m unittest scripts/test_retry_on_network_error.py -v
"""

from __future__ import annotations

import os
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "retry_on_network_error.sh"


class RetryOnNetworkErrorTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.counter = self.tmp / "attempts"
        self.counter.write_text("0")

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _fake_command(self, outputs: list[tuple[int, str]]) -> Path:
        """A command whose N-th invocation prints outputs[N][1] and exits outputs[N][0]."""
        cases = "\n".join(
            f"  {i + 1}) printf '%s\\n' {_sh_quote(text)}; exit {code} ;;"
            for i, (code, text) in enumerate(outputs)
        )
        path = self.tmp / "fake.sh"
        path.write_text(
            textwrap.dedent(
                """\
                #!/usr/bin/env bash
                n=$(( $(cat "{counter}") + 1 ))
                echo "$n" > "{counter}"
                case "$n" in
                {cases}
                  *) echo "unexpected attempt $n"; exit 99 ;;
                esac
                """
            ).format(counter=self.counter, cases=cases)
        )
        path.chmod(0o755)
        return path

    def _run(self, kind: str, command: Path, attempts: int = 3) -> subprocess.CompletedProcess:
        env = dict(
            os.environ,
            NETWORK_RETRY_ATTEMPTS=str(attempts),
            NETWORK_RETRY_BACKOFF_SECONDS="0",
        )
        return subprocess.run(
            ["bash", str(SCRIPT), kind, "--", str(command)],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

    def _attempts(self) -> int:
        return int(self.counter.read_text().strip())

    def test_success_runs_once(self) -> None:
        result = self._run("npm", self._fake_command([(0, "added 10 packages")]))
        self.assertEqual(result.returncode, 0)
        self.assertEqual(self._attempts(), 1)

    def test_npm_network_error_is_retried_until_success(self) -> None:
        cmd = self._fake_command(
            [
                (1, "npm error code ECONNRESET\nnpm error network aborted"),
                (1, "npm ERR! code E503"),
                (0, "added 10 packages"),
            ]
        )
        result = self._run("npm", cmd)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self._attempts(), 3)

    def test_npm_non_network_error_is_not_retried(self) -> None:
        cmd = self._fake_command([(1, "npm error code ERESOLVE\nnpm error ERESOLVE unable to resolve dependency tree")])
        result = self._run("npm", cmd)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self._attempts(), 1)
        self.assertIn("not a network/registry error", result.stderr)

    def test_npm_registry_404_is_not_retried(self) -> None:
        cmd = self._fake_command([(1, "npm error code E404\nnpm error 404 Not Found")])
        result = self._run("npm", cmd)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self._attempts(), 1)

    def test_npm_e5xx_prefix_does_not_match_longer_code(self) -> None:
        cmd = self._fake_command([(1, "npm error code E5000X")])
        result = self._run("npm", cmd)
        self.assertEqual(self._attempts(), 1)
        self.assertEqual(result.returncode, 1)

    def test_exhausted_attempts_return_last_status(self) -> None:
        cmd = self._fake_command([(7, "npm error code EAI_AGAIN")] * 3)
        result = self._run("npm", cmd, attempts=3)
        self.assertEqual(result.returncode, 7)
        self.assertEqual(self._attempts(), 3)
        self.assertIn("exhausted 3 attempts", result.stderr)

    def test_nuget_service_index_failure_is_retried(self) -> None:
        cmd = self._fake_command(
            [
                (1, "error NU1301: Unable to load the service index for source https://api.nuget.org/v3/index.json."),
                (0, "Restored dotnet/src/Snowflake.Data.csproj"),
            ]
        )
        result = self._run("nuget", cmd)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self._attempts(), 2)

    def test_nuget_missing_package_is_not_retried(self) -> None:
        cmd = self._fake_command([(1, "error NU1101: Unable to find package Foo. No packages exist with this id")])
        result = self._run("nuget", cmd)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self._attempts(), 1)

    def test_unknown_kind_is_a_usage_error(self) -> None:
        result = self._run("pip", self._fake_command([(0, "")]))
        self.assertEqual(result.returncode, 2)
        self.assertEqual(self._attempts(), 0)


def _sh_quote(text: str) -> str:
    return "'" + text.replace("'", "'\"'\"'") + "'"


if __name__ == "__main__":
    unittest.main()
