"""ODBC reference-driver coverage model — see models/__init__.py for the schema.

Each cell runs odbc_tests against the released Snowflake ODBC driver (the
reference driver) instead of the UD-ODBC build, consumed by
.github/workflows/_test-odbc-reference.yml. There is no cloud axis: the
reference job always decodes the default (aws) parameters.
"""

PARAMS = {
    "OS":   ["ubuntu", "macos", "windows"],
    "Arch": ["x64", "arm"],
    "DM":   ["unixodbc", "iodbc"],
}


def is_valid(c):
    """Block-list: return False to forbid a combo, fall through to allow."""
    if c["OS"] == "ubuntu":
        # The reference driver is installed from the Linux x64 package only.
        if c["Arch"] == "arm": return False

    if c["OS"] == "macos":
        # The macOS reference driver is a universal binary; it runs on the
        # macos-latest (arm) runner.
        if c["Arch"] == "x64": return False

    if c["OS"] == "windows":
        # The Windows reference installer and harness build are x64 only.
        if c["Arch"] == "arm": return False

    if c["DM"] == "iodbc":
        if c["OS"] != "macos": return False

    return True


CONSTRAINTS = [is_valid]

PR_CELLS = [
    {"OS": "ubuntu",  "Arch": "x64", "DM": "unixodbc"},
    {"OS": "macos",   "Arch": "arm", "DM": "unixodbc"},
    {"OS": "windows", "Arch": "x64", "DM": "unixodbc"},
    {"OS": "macos",   "Arch": "arm", "DM": "iodbc"},
]

# Windows x64 takes ~50 min (p50), longer than the rest of the merge queue
# combined, so the queue runs only the macOS iODBC lane (~15 min).
MERGE_QUEUE_CELLS = [
    {"OS": "macos", "Arch": "arm", "DM": "iodbc"},
]

JSON_CELLS = {"pr": [], "merge": [], "nightly": []}
