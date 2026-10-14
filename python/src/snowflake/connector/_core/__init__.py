"""Native core package.

Windows FIPS builds link shared AWS-LC libraries that sit next to the extension.
Python 3.8+ does not search that directory unless it is added explicitly.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

if sys.platform == "win32":
    # Held for the process lifetime. Collecting it removes the directory.
    _dll_directory = os.add_dll_directory(str(Path(__file__).resolve().parent))
