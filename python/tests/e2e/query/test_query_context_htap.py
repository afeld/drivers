"""
E2E tests: HTAP optimization preserves session metadata.

When ENABLE_SNOW_654741_FOR_TESTING is set, the server omits parameters and
metadata from SELECT responses. The driver must still report the correct
database, schema, warehouse, role, and session parameters from its client-side
cache.
"""

from __future__ import annotations

import time
import uuid

import pytest

from snowflake.connector.errors import ProgrammingError
from tests.e2e.put_get.put_get_helper import is_aws_test_account


# The first hybrid table in a database that has none takes several minutes.
# ht_sentinel stays so dropping the per-run table leaves one hybrid table behind.
QCC_TEST_DATABASES = ("qcc_test_db1", "qcc_test_db2")
QCC_SENTINEL_TABLE = "ht_sentinel"


def _use_database(cur, name: str) -> None:
    cur.execute(f"USE DATABASE {name}")
    # USE DATABASE leaves the previous schema selected. PUBLIC exists in every
    # database, so unqualified hybrid-table names resolve after the switch.
    cur.execute("USE SCHEMA PUBLIC")


def _ensure_hybrid_database(cur, name: str) -> None:
    cur.execute(f"CREATE DATABASE IF NOT EXISTS {name}")
    _use_database(cur, name)
    cur.execute(f"CREATE HYBRID TABLE IF NOT EXISTS {QCC_SENTINEL_TABLE} (id INT PRIMARY KEY)")


def _drop_hybrid_table_quiet(cur, database: str, table: str) -> None:
    try:
        _use_database(cur, database)
        cur.execute(f"DROP TABLE IF EXISTS {table}")
    except ProgrammingError:
        pass


@pytest.fixture
def htap_connection(connection_factory):
    """Yield a connection with ENABLE_SNOW_654741_FOR_TESTING enabled."""
    with connection_factory() as conn:
        with conn.cursor() as cur:
            cur.execute("ALTER SESSION SET ENABLE_SNOW_654741_FOR_TESTING = true")
        yield conn


class TestQueryContextHtap:
    # Scenario: should preserve schema after SELECT under HTAP optimization
    def test_should_preserve_schema_after_select_under_htap_optimization(
        self,
        htap_connection,
    ):
        conn = htap_connection
        # Given the account has ENABLE_SNOW_654741_FOR_TESTING enabled
        run_id = int(time.time() * 1000)
        new_schema = f"test_schema_{run_id}"
        assert conn.schema is None or not conn.schema.upper() == new_schema.upper()

        # When the client creates a new schema and executes SELECT
        with conn.cursor() as cur:
            cur.execute(f"CREATE SCHEMA {new_schema}")
            try:
                assert conn.schema.upper() == new_schema.upper()

                cur.execute("SELECT 1")

                # Then the connection still reports the new schema
                assert conn.schema.upper() == new_schema.upper(), (
                    f"Schema should survive SELECT, expected {new_schema}, got {conn.schema}"
                )
            finally:
                cur.execute(f"DROP SCHEMA IF EXISTS {new_schema}")

    # Scenario: should preserve database after SELECT under HTAP optimization
    def test_should_preserve_database_after_select_under_htap_optimization(
        self,
        htap_connection,
    ):
        conn = htap_connection
        # Given the account has ENABLE_SNOW_654741_FOR_TESTING enabled
        run_id = int(time.time() * 1000)
        new_database = f"test_database_{run_id}"
        assert conn.database is None or not conn.database.upper() == new_database.upper()

        # When the client creates a new database and executes SELECT
        with conn.cursor() as cur:
            cur.execute(f"CREATE DATABASE {new_database}")
            try:
                assert conn.database.upper() == new_database.upper()

                cur.execute("SELECT 1")

                # Then the connection still reports the new database
                assert conn.database.upper() == new_database.upper(), (
                    f"Database should survive SELECT, expected {new_database}, got {conn.database}"
                )
            finally:
                cur.execute(f"DROP DATABASE IF EXISTS {new_database}")

    # Scenario: should preserve role after SELECT under HTAP optimization
    def test_should_preserve_role_after_select_under_htap_optimization(
        self,
        htap_connection,
    ):
        conn = htap_connection
        # Given the account has ENABLE_SNOW_654741_FOR_TESTING enabled
        assert conn.role is None or not conn.role.upper() == "PUBLIC"

        # When the client switches to a different role and executes SELECT
        with conn.cursor() as cur:
            cur.execute("USE ROLE PUBLIC")
            assert conn.role.upper() == "PUBLIC"

            cur.execute("SELECT 1")

            # Then the connection still reports the switched role
            assert conn.role.upper() == "PUBLIC", f"Role should survive SELECT, expected PUBLIC, got {conn.role}"

    # Scenario: should preserve session parameter after SELECT under HTAP optimization
    def test_should_preserve_session_parameter_after_select_under_htap_optimization(
        self,
        htap_connection,
    ):
        conn = htap_connection
        # Given the account has ENABLE_SNOW_654741_FOR_TESTING enabled
        with conn.cursor() as cur:
            # When the client changes DATE_OUTPUT_FORMAT and executes SELECT
            cur.execute("ALTER SESSION SET DATE_OUTPUT_FORMAT = 'DD-MM-YYYY'")
            try:
                assert conn._session_parameters["DATE_OUTPUT_FORMAT"] == "DD-MM-YYYY"

                cur.execute("SELECT 1")

                # Then the session parameter still reflects the changed value
                assert conn._session_parameters["DATE_OUTPUT_FORMAT"] == "DD-MM-YYYY", (
                    f"DATE_OUTPUT_FORMAT should survive SELECT, got {conn._session_parameters['DATE_OUTPUT_FORMAT']}"
                )
            finally:
                cur.execute("ALTER SESSION SET DATE_OUTPUT_FORMAT = 'YYYY-MM-DD'")

    # Scenario: should operate on hybrid tables across multiple databases
    @pytest.mark.skipif(not is_aws_test_account(), reason="HTAP hybrid tables are enabled only on AWS")
    def test_should_operate_on_hybrid_tables_across_multiple_databases(
        self,
        connection_factory,
    ):
        # Given a connection to Snowflake
        db1, db2 = QCC_TEST_DATABASES
        table = f"ht_{uuid.uuid4().hex}"

        with connection_factory() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT CURRENT_DATABASE()")
                original_db = cur.fetchone()[0]

                try:
                    for database in QCC_TEST_DATABASES:
                        _ensure_hybrid_database(cur, database)

                    # When the client creates hybrid tables in two databases and inserts rows
                    _use_database(cur, db1)
                    cur.execute(f"CREATE HYBRID TABLE {table} (id INT PRIMARY KEY, text VARCHAR)")
                    cur.execute(f"INSERT INTO {table} VALUES (1, 'a')")

                    rows = cur.execute(f"SELECT * FROM {table}").fetchall()
                    assert rows == [(1, "a")]

                    cur.execute(f"INSERT INTO {table} VALUES (2, 'b')")
                    rows = cur.execute(f"SELECT * FROM {table} ORDER BY id").fetchall()
                    assert rows == [(1, "a"), (2, "b")]

                    _use_database(cur, db2)
                    cur.execute(f"CREATE HYBRID TABLE {table} (id INT PRIMARY KEY, text VARCHAR)")
                    cur.execute(f"INSERT INTO {table} VALUES (3, 'c')")

                    rows = cur.execute(f"SELECT * FROM {table}").fetchall()
                    assert rows == [(3, "c")]

                    _use_database(cur, db1)
                    cur.execute(f"INSERT INTO {table} VALUES (4, 'd')")

                    # Then selecting from each database returns the correct rows after switching back
                    rows = cur.execute(f"SELECT * FROM {table} ORDER BY id").fetchall()
                    assert len(rows) == 3
                    assert rows[0] == (1, "a")
                    assert rows[1] == (2, "b")
                    assert rows[2] == (4, "d")
                finally:
                    for database in QCC_TEST_DATABASES:
                        _drop_hybrid_table_quiet(cur, database, table)
                    if original_db:
                        try:
                            cur.execute(
                                "USE DATABASE IDENTIFIER(?)",
                                params=(original_db,),
                                _force_qmark_paramstyle=True,
                            )
                        except ProgrammingError:
                            pass
