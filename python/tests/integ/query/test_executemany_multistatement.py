"""executemany() applies num_statements to each parameter row, matching the v4 driver."""

from __future__ import annotations

import uuid

from tests.conftest import run_against_sync_and_async, with_paramstyle


pytestmark = run_against_sync_and_async


def _table(prefix: str) -> str:
    return f"{prefix}_{uuid.uuid4().hex[:8]}"


def _assert_each_statement_inserted_one_row(cursor) -> None:
    assert cursor.fetchone()[0] == 1
    while cursor.nextset():
        assert cursor.fetchone()[0] == 1


def _result_sets(cursor) -> list[list[tuple]]:
    sets = [list(cursor.fetchall())]
    while cursor.nextset():
        sets.append(list(cursor.fetchall()))
    return sets


@with_paramstyle("pyformat")
class TestExecutemanyNumStatementsPyformat:
    def test_named_params_insert_both_statements(self, cursor):
        table1 = _table("em_named_a")
        table2 = _table("em_named_b")
        cursor.execute(
            f"create temp table {table1} (id number, aa number); create temp table {table2} (id number, bb number);",
            num_statements=2,
        )
        cursor.executemany(
            f"insert into {table1}(id, aa) values(%(id)s, %(value1)s); "
            f"insert into {table2}(id, bb) values(%(id)s, %(value2)s);",
            [
                {"id": 1, "value1": 1234, "value2": 4},
                {"id": 2, "value1": 234, "value2": 34},
                {"id": 3, "value1": 34, "value2": 234},
                {"id": 4, "value1": 4, "value2": 1234},
            ],
            num_statements=2,
        )
        _assert_each_statement_inserted_one_row(cursor)
        cursor.execute(
            f"select aa from {table1} order by id; select bb from {table2} order by id;",
            num_statements=2,
        )
        assert _result_sets(cursor) == [
            [(1234,), (234,), (34,), (4,)],
            [(4,), (34,), (234,), (1234,)],
        ]

    def test_positional_params_insert_both_statements(self, cursor):
        table1 = _table("em_pos_a")
        table2 = _table("em_pos_b")
        cursor.execute(
            f"create temp table {table1} (id number, aa number); create temp table {table2} (id number, bb number);",
            num_statements=2,
        )
        cursor.executemany(
            f"insert into {table1}(id, aa) values(%s, %s); insert into {table2}(id, bb) values(%s, %s);",
            [
                (1, 12345, 1, 4),
                (2, 1234, 2, 34),
                (3, 234, 3, 234),
                (4, 34, 4, 1234),
                (5, 4, 5, 12345),
            ],
            num_statements=2,
        )
        _assert_each_statement_inserted_one_row(cursor)
        cursor.execute(
            f"select aa from {table1} order by id; select bb from {table2} order by id;",
            num_statements=2,
        )
        assert _result_sets(cursor) == [
            [(12345,), (1234,), (234,), (34,), (4,)],
            [(4,), (34,), (234,), (1234,), (12345,)],
        ]


@with_paramstyle("qmark")
class TestExecutemanyNumStatementsQmark:
    def test_qmark_params_insert_both_statements(self, cursor):
        table1 = _table("em_qmark_a")
        table2 = _table("em_qmark_b")
        cursor.execute(
            f"create temp table {table1} (id number, aa number); create temp table {table2} (id number, bb number);",
            num_statements=2,
        )
        cursor.executemany(
            f"insert into {table1}(id, aa) values(?, ?); insert into {table2}(id, bb) values(?, ?);",
            [
                [1, 1234, 1, 4],
                [2, 234, 2, 34],
                [3, 34, 3, 234],
                [4, 4, 4, 1234],
            ],
            num_statements=2,
        )
        _assert_each_statement_inserted_one_row(cursor)
        cursor.execute(
            f"select aa from {table1} order by id; select bb from {table2} order by id;",
            num_statements=2,
        )
        assert _result_sets(cursor) == [
            [(1234,), (234,), (34,), (4,)],
            [(4,), (34,), (234,), (1234,)],
        ]
