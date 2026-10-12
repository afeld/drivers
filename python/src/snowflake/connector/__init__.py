"""
PEP 249 Database API 2.0 Implementation

This module provides an empty implementation of the Python Database API Specification 2.0
as defined in PEP 249.
"""

from __future__ import annotations

from typing import Any, NamedTuple

from . import util_text  # noqa: F401 - backward compatibility re-exports
from ._internal.api_client.client_api import core_driver
from ._internal.decorators import pep249
from .connection import Connection, SnowflakeConnection
from .connection_config import ConnectionConfig
from .constants import QueryStatus, StatementParameterName
from .cursor import DictCursor, SnowflakeCursor
from .errors import (
    DatabaseError,
    DataError,
    Error,
    IntegrityError,
    InterfaceError,
    InternalError,
    NotSupportedError,
    OperationalError,
    ProgrammingError,
    Warning,
)
from .types import (
    BINARY,
    DATETIME,
    NUMBER,
    ROWID,
    STRING,
    Binary,
    Date,
    DateFromTicks,
    Time,
    TimeFromTicks,
    Timestamp,
    TimestampFromTicks,
)
from .version import __version__


# PEP 249 Module Interface Constants
apilevel = "2.0"
threadsafety = 2  # Threads may share the module and connections, but not cursors
paramstyle = "pyformat"  # Default: %(name)s and %s placeholders (client-side interpolation)


class FipsStatus(NamedTuple):
    """Whether this process is the FIPS driver.

    ``is_fips`` is true when this binary was built with ``fips`` and its linked
    TLS provider is in FIPS mode. A standard build returns false.
    """

    is_fips: bool


# Sentinel to distinguish "not provided" from explicit values. Forwarding ``None``
# defaults would make ``@api_telemetry`` treat connection_name/config as passed.
_UNSET = object()


@pep249
def connect(
    *,
    connection_name: str | None | object = _UNSET,
    connections_file_path: str | None | object = _UNSET,
    config: ConnectionConfig | None | object = _UNSET,
    **kwargs: Any,
) -> Connection:
    """
    Create a connection to the database.

    Args:
        database: Database name
        user: Username
        password: Password
        host: Host name
        port: Port number
        **kwargs: Additional connection parameters

    Returns:
        Connection: A Connection object
    """
    conn_kwargs = dict(kwargs)
    if connection_name is not _UNSET:
        conn_kwargs["connection_name"] = connection_name
    if connections_file_path is not _UNSET:
        conn_kwargs["connections_file_path"] = connections_file_path
    if config is not _UNSET:
        conn_kwargs["config"] = config
    return Connection(**conn_kwargs)


@pep249
def get_fips_status() -> FipsStatus:
    """Report whether this process is the FIPS driver without opening a connection."""
    response = core_driver.driver_get_fips_status()
    return FipsStatus(is_fips=response.is_fips)


# Export all public symbols
__all__ = [
    # Version
    "__version__",
    # Module constants
    "apilevel",
    "threadsafety",
    "paramstyle",
    # Module functions
    "connect",
    "get_fips_status",
    # Classes
    "FipsStatus",
    "ConnectionConfig",
    "Connection",
    "SnowflakeConnection",
    "QueryStatus",
    "StatementParameterName",
    "DictCursor",
    "SnowflakeCursor",
    # Exceptions
    "Warning",
    "Error",
    "InterfaceError",
    "DatabaseError",
    "DataError",
    "OperationalError",
    "IntegrityError",
    "InternalError",
    "ProgrammingError",
    "NotSupportedError",
    # Type constructors
    "Date",
    "Time",
    "Timestamp",
    "DateFromTicks",
    "TimeFromTicks",
    "TimestampFromTicks",
    "Binary",
    # Type objects
    "STRING",
    "BINARY",
    "NUMBER",
    "DATETIME",
    "ROWID",
]
