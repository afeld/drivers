#include <algorithm>
#include <cstring>
#include <string>
#include <vector>

#include <catch2/catch_test_macros.hpp>

#include "Connection.hpp"
#include "SchemaFixtures.hpp"
#include "compatibility.hpp"
#include "get_data.hpp"
#include "get_diag_rec.hpp"
#include "odbc_matchers.hpp"

// =============================================================================
// Tests for SQLNumResultCols based on ODBC specification:
// https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function
// =============================================================================

// =============================================================================
// Happy Path
// =============================================================================

TEST_CASE("SQLNumResultCols returns 1 for SELECT with single column.", "[query]") {
  // Doc: "SQLNumResultCols returns the number of columns in a result set."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#summary

  // Given Snowflake client is logged in
  Connection conn;

  // When a SELECT query with one column is executed
  auto stmt = conn.execute("SELECT 42 AS value");

  // Then SQLNumResultCols should return 1
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 1);
}

TEST_CASE("SQLNumResultCols returns correct count for SELECT with many columns.", "[query]") {
  // Doc: "SQLNumResultCols returns the number of columns in a result set."
  // Doc: "This count does not include a bound bookmark column."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#arguments

  // Given Snowflake client is logged in
  Connection conn;

  // When a SELECT query with 5 columns is executed
  auto stmt = conn.execute("SELECT 1 AS a, 2 AS b, 3 AS c, 4 AS d, 5 AS e");

  // Then SQLNumResultCols should return 5
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 5);
}

TEST_CASE_METHOD(ConnSchemaFixture, "SQLNumResultCols returns correct count for SELECT * from table.", "[query]") {
  // Doc: "SQLNumResultCols returns the number of columns in a result set."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#summary

  // Given Snowflake client is logged in

  // And a table with 3 columns exists
  conn.execute("CREATE TEMPORARY TABLE num_cols_test (id INT, name VARCHAR(100), active BOOLEAN)");

  // When SELECT * is executed on the table
  auto stmt = conn.execute("SELECT * FROM num_cols_test");

  // Then SQLNumResultCols should return 3
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 3);
}

// =============================================================================
// Empty Result Set
// =============================================================================

TEST_CASE("SQLNumResultCols returns correct column count for empty result set.", "[query]") {
  // Doc: "SQLNumResultCols returns the number of columns in a result set."
  // Doc: "If the statement associated with StatementHandle does not return columns,
  //       SQLNumResultCols sets *ColumnCountPtr to 0."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#comments

  // Given Snowflake client is logged in
  Connection conn;

  // When a SELECT query with WHERE 1=0 is executed (empty result set but with columns)
  auto stmt = conn.execute("SELECT 1 AS col1, 2 AS col2 WHERE 1=0");

  // Then SQLNumResultCols should still return the column count (2)
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 2);
}

// =============================================================================
// DDL / No Result Set
// =============================================================================

TEST_CASE_METHOD(ConnSchemaFixture, "SQLNumResultCols returns 0 after DDL statement.", "[query]") {
  // Doc: "If the statement associated with StatementHandle does not return columns,
  //       SQLNumResultCols sets *ColumnCountPtr to 0."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#comments

  // Given Snowflake client is logged in
  const auto stmt = conn.createStatement();

  // When a DDL statement is executed
  SQLRETURN ret = SQLExecDirect(stmt.getHandle(), (SQLCHAR*)"CREATE TABLE ddl_numcols_test (id INT)", SQL_NTS);
  REQUIRE_ODBC(ret, stmt);

  // Then SQLNumResultCols should return 0 (DDL produces no result set columns)
  SQLSMALLINT num_cols = -1;
  ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 0);
}

TEST_CASE_METHOD(ConnSchemaFixture, "SQLNumResultCols returns correct count after calling a stored procedure.",
                 "[query]") {
  // Given Snowflake client is logged in

  // And a stored procedure exists that returns one column
  conn.execute(
      "CREATE PROCEDURE numcols_proc(p1 VARCHAR) "
      "RETURNS VARCHAR LANGUAGE SQL AS 'BEGIN RETURN p1; END'");

  // When the stored procedure is called
  auto stmt = conn.execute("CALL numcols_proc('hello')");

  // Then SQLNumResultCols should return 1
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 1);
}

// =============================================================================
// State Errors
// =============================================================================

TEST_CASE("SQLNumResultCols returns HY010 when called on freshly allocated statement.", "[query]") {
  // Doc: "(DM) The function was called prior to calling SQLPrepare or SQLExecDirect
  //       for the StatementHandle."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#diagnostics

  // Given Snowflake client is logged in
  Connection conn;
  auto stmt = conn.createStatement();

  // When SQLNumResultCols is called without prior SQLPrepare or SQLExecDirect
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);

  // Then SQLNumResultCols returns SQL_ERROR under every DM
  REQUIRE(ret == SQL_ERROR);
  NON_IODBC {
    // And the diagnostic is the ODBC 3.x SQLSTATE HY010
    //   (function sequence error)
    CHECK(get_sqlstate(stmt) == "HY010");
  }
  IODBC_ONLY {
    // And the diagnostic is the ODBC 2.x SQLSTATE S1010 (same condition,
    //   older code) because the DM has not remapped the legacy state
    CHECK(get_sqlstate(stmt) == "S1010");
  }
}

// =============================================================================
// After SQLPrepare (before SQLExecute)
// =============================================================================

TEST_CASE("SQLNumResultCols returns column count after SQLPrepare.", "[query]") {
  // Doc: "SQLNumResultCols can be called successfully only when the statement is
  //       in the prepared, executed, or positioned state."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#comments

  // Given Snowflake client is logged in
  Connection conn;
  auto stmt = conn.createStatement();

  // When a SELECT statement is prepared but not executed
  SQLRETURN ret = SQLPrepare(stmt.getHandle(), (SQLCHAR*)"SELECT 1 AS a, 2 AS b, 3 AS c", SQL_NTS);
  REQUIRE_ODBC(ret, stmt);

  // Then SQLNumResultCols should return the column count
  SQLSMALLINT num_cols = 0;
  ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 3);
}

// =============================================================================
// Re-execution
// =============================================================================

TEST_CASE("SQLNumResultCols updates column count after re-execution with different query.", "[query]") {
  // Doc: "SQLNumResultCols returns the number of columns in a result set."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#summary

  // Given Snowflake client is logged in
  Connection conn;
  auto stmt = conn.createStatement();

  // When a query with 3 columns is executed
  SQLRETURN ret = SQLExecDirect(stmt.getHandle(), (SQLCHAR*)"SELECT 1 AS a, 2 AS b, 3 AS c", SQL_NTS);
  REQUIRE_ODBC(ret, stmt);

  // And SQLNumResultCols is called
  SQLSMALLINT num_cols = 0;
  ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 3);

  // And the cursor is closed before re-executing
  ret = SQLCloseCursor(stmt.getHandle());
  REQUIRE_ODBC(ret, stmt);

  // And a different query with 1 column is executed on the same statement
  ret = SQLExecDirect(stmt.getHandle(), (SQLCHAR*)"SELECT 42 AS only_col", SQL_NTS);
  REQUIRE_ODBC(ret, stmt);

  // Then SQLNumResultCols should return the updated column count
  ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 1);
}

// =============================================================================
// IRD Consistency
// =============================================================================

TEST_CASE("SQLNumResultCols returns same value as SQL_DESC_COUNT of the IRD.", "[query]") {
  // Doc: "The number of columns returned by SQLNumResultCols is the same value
  //       as the SQL_DESC_COUNT field of the IRD."
  // https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlnumresultcols-function#comments

  // Given Snowflake client is logged in
  Connection conn;
  auto stmt = conn.createStatement();

  // When a query with 4 columns is executed
  SQLRETURN ret = SQLExecDirect(stmt.getHandle(), (SQLCHAR*)"SELECT 1 AS a, 2 AS b, 3 AS c, 4 AS d", SQL_NTS);
  REQUIRE_ODBC(ret, stmt);

  // And SQLNumResultCols is called
  SQLSMALLINT num_cols = 0;
  ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);

  // And the IRD SQL_DESC_COUNT is read
  SQLHDESC ird = SQL_NULL_HDESC;
  ret = SQLGetStmtAttr(stmt.getHandle(), SQL_ATTR_IMP_ROW_DESC, &ird, 0, NULL);
  REQUIRE(ret == SQL_SUCCESS);

  SQLSMALLINT ird_count = 0;
  ret = SQLGetDescField(ird, 0, SQL_DESC_COUNT, &ird_count, 0, NULL);
  REQUIRE(ret == SQL_SUCCESS);

  // Then both values should be equal
  CHECK(num_cols == ird_count);
  CHECK(num_cols == 4);
}

// =============================================================================
// UNLOAD (COPY INTO location)
// =============================================================================

TEST_CASE_METHOD(ConnSchemaFixture, "SQLNumResultCols returns 3 for COPY INTO stage with DETAILED_OUTPUT.", "[query]") {
  // Given Snowflake client is logged in

  // And a temporary stage and a table with 3 rows exist
  conn.execute("CREATE OR REPLACE TEMPORARY STAGE unload_numcols_stage");
  conn.execute("CREATE TEMPORARY TABLE unload_numcols_src (id INT)");
  conn.execute("INSERT INTO unload_numcols_src VALUES (1), (2), (3)");

  // When the table is unloaded into one file with DETAILED_OUTPUT = TRUE
  auto stmt = conn.execute(
      "COPY INTO @unload_numcols_stage FROM unload_numcols_src "
      "FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE "
      "OVERWRITE = TRUE SINGLE = TRUE");

  // Then SQLNumResultCols returns the DETAILED_OUTPUT column count
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 3);

  // And SQLFetch succeeds, FILE_NAME is non-empty, and ROW_COUNT equals the source row count
  ret = SQLFetch(stmt.getHandle());
  REQUIRE_ODBC(ret, stmt);
  CHECK(!get_data<SQL_C_CHAR>(stmt, 1).empty());
  CHECK(get_data<SQL_C_SBIGINT>(stmt, 3) == 3);

  ret = SQLFetch(stmt.getHandle());
  REQUIRE(ret == SQL_NO_DATA);
}

TEST_CASE_METHOD(
    ConnSchemaFixture,
    "SQLNumResultCols returns DETAILED_OUTPUT columns and one row per file for partitioned COPY INTO unload.",
    "[query]") {
  // Given Snowflake client is logged in

  // And a temporary stage and a table with 3 rows in partition a and 4 rows in partition b exist
  conn.execute("CREATE OR REPLACE TEMPORARY STAGE unload_part_stage");
  conn.execute("CREATE TEMPORARY TABLE unload_part_src (grp VARCHAR, id INT)");
  conn.execute(
      "INSERT INTO unload_part_src VALUES ('a', 1), ('a', 2), ('a', 3), ('b', 4), ('b', 5), ('b', 6), ('b', 7)");

  // When the table is unloaded with PARTITION BY and DETAILED_OUTPUT = TRUE
  auto stmt = conn.execute(
      "COPY INTO @unload_part_stage FROM unload_part_src "
      "PARTITION BY ('p=' || grp) "
      "FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE");

  // Then SQLNumResultCols returns the partitioned DETAILED_OUTPUT column count
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 4);

  // And the columns are named PARTITION_NAME, FILE_NAME, FILE_SIZE, ROW_COUNT
  std::vector<std::string> column_names;
  for (SQLUSMALLINT col = 1; col <= static_cast<SQLUSMALLINT>(num_cols); ++col) {
    SQLCHAR name[64] = {};
    SQLSMALLINT name_len = 0;
    ret = SQLDescribeCol(stmt.getHandle(), col, name, sizeof(name), &name_len, nullptr, nullptr, nullptr, nullptr);
    REQUIRE_ODBC(ret, stmt);
    column_names.emplace_back(reinterpret_cast<char*>(name), name_len);
  }
  CHECK(column_names == std::vector<std::string>{"PARTITION_NAME", "FILE_NAME", "FILE_SIZE", "ROW_COUNT"});

  // And one row per partition file is fetched, with ROW_COUNT values 3 and 4
  std::vector<SQLBIGINT> row_counts;
  for (;;) {
    ret = SQLFetch(stmt.getHandle());
    if (ret == SQL_NO_DATA) {
      break;
    }
    REQUIRE_ODBC(ret, stmt);
    CHECK(!get_data<SQL_C_CHAR>(stmt, 2).empty());
    row_counts.push_back(get_data<SQL_C_SBIGINT>(stmt, 4));
  }
  std::sort(row_counts.begin(), row_counts.end());
  CHECK(row_counts == std::vector<SQLBIGINT>{3, 4});
}

TEST_CASE_METHOD(ConnSchemaFixture,
                 "SQLNumResultCols returns 3 and SQLFetch returns no rows for an empty COPY INTO unload.", "[query]") {
  // Given Snowflake client is logged in

  // And a temporary stage and an empty table exist
  conn.execute("CREATE OR REPLACE TEMPORARY STAGE unload_empty_stage");
  conn.execute("CREATE TEMPORARY TABLE unload_empty_src (id INT)");

  // When the empty table is unloaded into one file with DETAILED_OUTPUT = TRUE
  auto stmt = conn.execute(
      "COPY INTO @unload_empty_stage FROM unload_empty_src "
      "FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE "
      "OVERWRITE = TRUE SINGLE = TRUE");

  // Then SQLNumResultCols returns the DETAILED_OUTPUT column count
  SQLSMALLINT num_cols = 0;
  SQLRETURN ret = SQLNumResultCols(stmt.getHandle(), &num_cols);
  REQUIRE_ODBC(ret, stmt);
  CHECK(num_cols == 3);

  // And SQLFetch returns SQL_NO_DATA because no file was written
  ret = SQLFetch(stmt.getHandle());
  REQUIRE(ret == SQL_NO_DATA);
}

TEST_CASE_METHOD(ConnSchemaFixture, "SQLExecDirect fails with native error 2003 for COPY INTO a missing stage.",
                 "[query]") {
  // Given Snowflake client is logged in

  // And a table with one row exists and the stage does not
  conn.execute("CREATE TEMPORARY TABLE unload_missing_stage_src (id INT)");
  conn.execute("INSERT INTO unload_missing_stage_src VALUES (1)");
  auto stmt = conn.createStatement();

  // When the table is unloaded into the missing stage
  SQLRETURN ret = SQLExecDirect(stmt.getHandle(),
                                sqlchar("COPY INTO @unload_missing_stage FROM unload_missing_stage_src "
                                        "FILE_FORMAT = (TYPE = CSV)"),
                                SQL_NTS);
  const OdbcResult result(ret, stmt);

  // Then SQL_ERROR is returned with native error 2003 and a message naming the stage
  REQUIRE_THAT(result, OdbcMatchers::IsError() && OdbcMatchers::HasNativeError(2003) &&
                           OdbcMatchers::HasDiagMessage("UNLOAD_MISSING_STAGE"));

  OLD_DRIVER_ONLY("BD#172") {
    // And the SQLSTATE is the server's 02000
    CHECK_THAT(result, OdbcMatchers::HasSqlState("02000"));
  }
  NEW_DRIVER_ONLY("BD#172") {
    // And the SQLSTATE is HY000 because class 02 is reserved for SQL_NO_DATA
    CHECK_THAT(result, OdbcMatchers::HasSqlState("HY000"));
  }
}
