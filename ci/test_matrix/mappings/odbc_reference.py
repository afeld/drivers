"""
ODBC reference-driver mapping table.

Per-row keys:
  label       (str, required) Display name used in the job name
              `odbc_reference_tests (<label>)`. A non-default driver manager
              appends ` + <DM>` (e.g. "macOS Universal + iODBC").
  driver_lib  (str, required) File name of the released driver library.
"""

ODBC_REFERENCE_PLATFORM: dict[tuple[str, str], dict[str, str]] = {
    ("ubuntu",  "x64"): {"label": "Linux x64",       "driver_lib": "libsfodbc.so"},
    ("macos",   "arm"): {"label": "macOS Universal", "driver_lib": "libSnowflake.dylib"},
    ("windows", "x64"): {"label": "Windows x64",     "driver_lib": "sfodbc.dll"},
}
