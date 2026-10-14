"""FIPS status for this process.

``snowflake.connector`` re-exports :class:`FipsStatus` and :func:`get_fips_status`.
"""

from __future__ import annotations

from typing import NamedTuple

from .api_client.client_api import core_driver
from .decorators import pep249


class FipsStatus(NamedTuple):
    """Whether this process is the FIPS driver.

    ``is_fips`` is true when this binary was built with ``fips`` and its linked
    TLS provider is in FIPS mode. A standard build returns false.
    """

    is_fips: bool


@pep249
def get_fips_status() -> FipsStatus:
    """Report whether this process is the FIPS driver without opening a connection."""
    response = core_driver.driver_get_fips_status()
    return FipsStatus(is_fips=response.is_fips)
