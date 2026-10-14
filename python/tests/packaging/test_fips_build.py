"""SNOWFLAKE_FIPS=1 links the FIPS module only for an unpacked sdist."""

from __future__ import annotations

import os
import platform
import subprocess
import sys
import venv

from pathlib import Path

import pytest

from hatch_build import BuildHook


def get_python_dir() -> Path:
    """Return the python/ project root (parent of tests/)."""
    return Path(__file__).parent.parent.parent


# A cold FIPS release build compiles aws-lc-fips and routinely exceeds the
# default packaging timeout. Configurable so CI can raise it without a code change.
FIPS_INSTALL_TIMEOUT = int(os.environ.get("FIPS_INSTALL_TIMEOUT", "3300"))
# A cold release build of the standard module exceeds the default packaging
# timeout. SDIST_INSTALL_TIMEOUT raises it without a code change.
STANDARD_INSTALL_TIMEOUT = int(os.environ.get("SDIST_INSTALL_TIMEOUT", "1500"))


def _python_in(venv_path: Path) -> Path:
    if sys.platform == "win32":
        return venv_path / "Scripts" / "python.exe"
    return venv_path / "bin" / "python"


def _run_command(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: int = 300,
) -> subprocess.CompletedProcess:
    full_env = os.environ.copy()
    if env:
        full_env.update(env)
    return subprocess.run(
        cmd,
        cwd=cwd,
        env=full_env,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


@pytest.mark.parametrize("target_name", ["wheel", "editable"])
def test_snowflake_fips_outside_sdist_fails(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, target_name: str) -> None:
    """A checkout, editable install, or wheel build fails when SNOWFLAKE_FIPS is set."""
    monkeypatch.setenv("SNOWFLAKE_FIPS", "1")
    hook = BuildHook(str(tmp_path), {}, None, None, str(tmp_path), target_name)
    with pytest.raises(RuntimeError, match="unpacked sdist"):
        hook._fips_requested()


def test_snowflake_fips_on_unpacked_sdist_links_fips(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """PKG-INFO is what makes an unpacked sdist honor SNOWFLAKE_FIPS."""
    monkeypatch.setenv("SNOWFLAKE_FIPS", "true")
    (tmp_path / "PKG-INFO").write_text("Metadata-Version: 2.1\n", encoding="utf-8")
    hook = BuildHook(str(tmp_path), {}, None, None, str(tmp_path), "wheel")
    assert hook._fips_requested() is True


def test_snowflake_fips_unset_links_standard_module(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """An unset variable does not request the FIPS module."""
    monkeypatch.delenv("SNOWFLAKE_FIPS", raising=False)
    hook = BuildHook(str(tmp_path), {}, None, None, str(tmp_path), "wheel")
    assert hook._fips_requested() is False


def _build_sdist(dist_dir: Path) -> Path:
    dist_dir.mkdir()
    built = _run_command(
        [sys.executable, "-m", "build", "--sdist", "--installer", "uv", "--outdir", str(dist_dir)],
        cwd=get_python_dir(),
        timeout=600,
    )
    if built.returncode != 0:
        pytest.fail(f"sdist build failed:\nstdout: {built.stdout}\nstderr: {built.stderr}")
    sdists = list(dist_dir.glob("*.tar.gz"))
    if len(sdists) != 1:
        pytest.fail(f"Expected one sdist in {dist_dir}, found {sdists}")
    return sdists[0]


def _venv_python(venv_path: Path) -> Path:
    # uv's macOS CPython loads libpython from @executable_path/../lib, so a
    # copied interpreter aborts in ensurepip. Windows does not use that path.
    venv.create(venv_path, with_pip=True, symlinks=sys.platform != "win32")
    python_exe = _python_in(venv_path)
    upgraded = _run_command([str(python_exe), "-m", "pip", "install", "--upgrade", "pip"])
    if upgraded.returncode != 0:
        pytest.fail(f"pip upgrade failed:\nstdout: {upgraded.stdout}\nstderr: {upgraded.stderr}")
    return python_exe


def _install_sdist_and_read_is_fips(
    python_exe: Path,
    sdist: Path,
    *,
    install_env: dict[str, str],
    timeout: int,
) -> bool:
    installed = _run_command(
        [str(python_exe), "-m", "pip", "install", str(sdist), "--verbose"],
        env=install_env,
        timeout=timeout,
    )
    if installed.returncode != 0:
        pytest.fail(f"pip install failed:\nstdout: {installed.stdout}\nstderr: {installed.stderr}")

    probe = _run_command(
        [
            str(python_exe),
            "-c",
            (
                "from snowflake.connector import get_fips_status\n"
                "status = get_fips_status()\n"
                "print('true' if status.is_fips else 'false')\n"
            ),
        ]
    )
    if probe.returncode != 0:
        pytest.fail(f"get_fips_status() failed:\nstdout:\n{probe.stdout}\nstderr:\n{probe.stderr}")
    reported = probe.stdout.strip().splitlines()[-1]
    if reported not in {"true", "false"}:
        pytest.fail(f"get_fips_status() did not print is_fips:\nstdout:\n{probe.stdout}\nstderr:\n{probe.stderr}")
    return reported == "true"


# Empty SKIP_CORE_BUILD overrides a parent "true", so the install compiles the core.
# Empty SF_PERF_METRICS leaves the other opt-in feature off, so the artifact is this build alone.
_CORE_INSTALL_ENV = {
    "SKIP_CORE_BUILD": "",
    "SF_PERF_METRICS": "",
}


@pytest.mark.slow
@pytest.mark.timeout(2400)
def test_standard_sdist_artifact_reports_is_fips_false(tmp_path: Path) -> None:
    """An sdist install without SNOWFLAKE_FIPS reports is_fips as false."""
    # Empty SNOWFLAKE_FIPS overrides a parent "1", so this install links the standard module.
    is_fips = _install_sdist_and_read_is_fips(
        _venv_python(tmp_path / "venv"),
        _build_sdist(tmp_path / "dist"),
        install_env={**_CORE_INSTALL_ENV, "SNOWFLAKE_FIPS": ""},
        timeout=STANDARD_INSTALL_TIMEOUT,
    )
    assert is_fips is False


@pytest.mark.slow
@pytest.mark.timeout(7200)
@pytest.mark.skipif(
    sys.platform == "win32" and platform.machine().upper() in {"ARM64", "AARCH64"},
    reason="aws-lc-fips-sys does not build on Windows ARM64",
)
def test_snowflake_fips_artifact_reports_fips_mode(tmp_path: Path) -> None:
    """An sdist install with SNOWFLAKE_FIPS=1 reports is_fips as true."""
    is_fips = _install_sdist_and_read_is_fips(
        _venv_python(tmp_path / "venv"),
        _build_sdist(tmp_path / "dist"),
        install_env={**_CORE_INSTALL_ENV, "SNOWFLAKE_FIPS": "1"},
        timeout=FIPS_INSTALL_TIMEOUT,
    )
    assert is_fips is True
