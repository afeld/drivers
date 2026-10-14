"""COPY INTO location (UNLOAD) result and rowcount behavior."""

from __future__ import annotations

import uuid

import pytest

from snowflake.connector import ProgrammingError

from ...conftest import with_paramstyle


def _object_name(prefix: str) -> str:
    return f"{prefix}_{uuid.uuid4().hex[:8]}".upper()


@with_paramstyle("qmark")
class TestUnload:
    """COPY INTO <location> returns status rows and a file-count rowcount."""

    def test_should_return_detailed_output_rows_and_file_count_for_copy_into_stage(self, cursor):
        """One SINGLE file reports its source row count, and rowcount is 1."""
        # Given Snowflake client is logged in
        # And a temporary stage and a table with 3 rows exist
        stage = _object_name("UNLOAD_STAGE")
        table = _object_name("UNLOAD_SRC")
        cursor.execute(
            "CREATE OR REPLACE TEMPORARY STAGE IDENTIFIER(?)",
            params=(stage,),
        )
        cursor.execute(
            "CREATE TEMPORARY TABLE IDENTIFIER(?) (id INT)",
            params=(table,),
        )
        cursor.execute(
            "INSERT INTO IDENTIFIER(?) VALUES (1), (2), (3)",
            params=(table,),
        )

        # When the table is unloaded into one file with DETAILED_OUTPUT = TRUE
        # @stage is not a bindable COPY INTO location. The name is generated in this test.
        cursor.execute(
            f"COPY INTO @{stage} FROM IDENTIFIER(?) "
            "FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE "
            "OVERWRITE = TRUE SINGLE = TRUE",
            params=(table,),
        )

        # Then the result has FILE_NAME, FILE_SIZE, ROW_COUNT and one row whose ROW_COUNT is 3
        assert cursor.description is not None
        assert [column.name for column in cursor.description] == [
            "FILE_NAME",
            "FILE_SIZE",
            "ROW_COUNT",
        ]
        rows = cursor.fetchall()
        assert len(rows) == 1
        assert rows[0][0]
        assert rows[0][2] == 3
        # And rowcount is the file count, not the 3 source rows
        assert cursor.rowcount == 1

    def test_should_return_one_row_per_file_and_file_count_for_partitioned_copy_into_stage(self, cursor):
        """Partitioned unload reports one row per file and rowcount equal to that file count."""
        # Given Snowflake client is logged in
        # And a temporary stage and a table with 3 rows in partition a and 4 rows in partition b exist
        stage = _object_name("UNLOAD_PART_STAGE")
        table = _object_name("UNLOAD_PART_SRC")
        cursor.execute(
            "CREATE OR REPLACE TEMPORARY STAGE IDENTIFIER(?)",
            params=(stage,),
        )
        cursor.execute(
            "CREATE TEMPORARY TABLE IDENTIFIER(?) (grp VARCHAR, id INT)",
            params=(table,),
        )
        cursor.execute(
            "INSERT INTO IDENTIFIER(?) VALUES ('a', 1), ('a', 2), ('a', 3), ('b', 4), ('b', 5), ('b', 6), ('b', 7)",
            params=(table,),
        )

        # When the table is unloaded with PARTITION BY and DETAILED_OUTPUT = TRUE
        # @stage is not a bindable COPY INTO location. The name is generated in this test.
        cursor.execute(
            f"COPY INTO @{stage} FROM IDENTIFIER(?) "
            "PARTITION BY ('p=' || grp) "
            "FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE",
            params=(table,),
        )

        # Then the columns are PARTITION_NAME, FILE_NAME, FILE_SIZE, ROW_COUNT
        assert cursor.description is not None
        assert [column.name for column in cursor.description] == [
            "PARTITION_NAME",
            "FILE_NAME",
            "FILE_SIZE",
            "ROW_COUNT",
        ]
        rows = cursor.fetchall()
        assert rows
        assert all(row[0] and row[1] for row in rows)
        # And one row per file is returned, with ROW_COUNT values 3 and 4
        assert sorted(row[3] for row in rows) == [3, 4]
        # And rowcount is the file count, not the 7 source rows
        assert cursor.rowcount == 2

    def test_should_return_no_file_rows_and_zero_rowcount_when_unloading_an_empty_table(self, cursor):
        """An empty source table writes no file, so there are no status rows."""
        # Given Snowflake client is logged in
        # And a temporary stage and an empty table exist
        stage = _object_name("UNLOAD_EMPTY_STAGE")
        table = _object_name("UNLOAD_EMPTY_SRC")
        cursor.execute(
            "CREATE OR REPLACE TEMPORARY STAGE IDENTIFIER(?)",
            params=(stage,),
        )
        cursor.execute(
            "CREATE TEMPORARY TABLE IDENTIFIER(?) (id INT)",
            params=(table,),
        )

        # When the empty table is unloaded into one file with DETAILED_OUTPUT = TRUE
        # @stage is not a bindable COPY INTO location. The name is generated in this test.
        cursor.execute(
            f"COPY INTO @{stage} FROM IDENTIFIER(?) "
            "FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE "
            "OVERWRITE = TRUE SINGLE = TRUE",
            params=(table,),
        )

        # Then the columns are FILE_NAME, FILE_SIZE, ROW_COUNT and no file rows are returned
        assert cursor.description is not None
        assert [column.name for column in cursor.description] == [
            "FILE_NAME",
            "FILE_SIZE",
            "ROW_COUNT",
        ]
        assert cursor.fetchall() == []
        # And rowcount is 0
        assert cursor.rowcount == 0

    def test_should_fail_when_copy_into_stage_does_not_exist(self, cursor):
        """COPY INTO a missing stage fails with the server object-not-found error."""
        # Given Snowflake client is logged in
        # And a table with one row exists and the stage does not
        stage = _object_name("MISSING_UNLOAD_STAGE")
        table = _object_name("UNLOAD_SRC")
        cursor.execute(
            "CREATE TEMPORARY TABLE IDENTIFIER(?) (id INT)",
            params=(table,),
        )
        cursor.execute(
            "INSERT INTO IDENTIFIER(?) VALUES (1)",
            params=(table,),
        )

        # When the table is unloaded into the missing stage
        # Then the unload fails with errno 2003 and SQLSTATE 02000 naming the stage
        with pytest.raises(ProgrammingError) as excinfo:
            # @stage is not a bindable COPY INTO location. The name is generated in this test.
            cursor.execute(
                f"COPY INTO @{stage} FROM IDENTIFIER(?) FILE_FORMAT = (TYPE = CSV)",
                params=(table,),
            )
        error = excinfo.value
        assert error.errno == 2003
        assert error.sqlstate == "02000"
        assert stage in error.msg
        assert "does not exist" in error.msg
