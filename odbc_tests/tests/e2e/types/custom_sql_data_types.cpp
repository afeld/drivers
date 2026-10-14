#include <sql.h>
#include <sqlext.h>
#include <sqltypes.h>

#include <string>

#include <catch2/catch_test_macros.hpp>

#include "Connection.hpp"
#include "geo_test_helpers.hpp"
#include "sf_odbc.h"

TEST_CASE("should report geography concise type sql_type for format when ODBC_USE_CUSTOM_SQL_DATA_TYPES is flag_state",
          "[geography]") {
  // Given Snowflake client is logged in
  Connection conn;

  struct Case {
    const char* format;
    const char* flag_state;
    SQLSMALLINT sql_type;
  };
  const Case cases[] = {
      {"GeoJSON", "FALSE", SQL_VARCHAR},
      {"WKB", "FALSE", SQL_BINARY},
      {"GeoJSON", "TRUE", SQL_SF_OBJECT},
      {"WKB", "TRUE", SQL_BINARY},
  };

  for (const auto& test_case : cases) {
    INFO(test_case.format << " ODBC_USE_CUSTOM_SQL_DATA_TYPES=" << test_case.flag_state);

    // And Session parameter ODBC_USE_CUSTOM_SQL_DATA_TYPES is <flag_state>
    conn.execute(std::string("ALTER SESSION SET ODBC_USE_CUSTOM_SQL_DATA_TYPES = ") + test_case.flag_state);

    // And Session parameter GEOGRAPHY_OUTPUT_FORMAT is set to <format>
    conn.execute(std::string("ALTER SESSION SET GEOGRAPHY_OUTPUT_FORMAT = '") + test_case.format + "'");

    // When Query "SELECT TO_GEOGRAPHY('POINT(-122.35 37.55)')" is executed
    const auto stmt = conn.execute_fetch("SELECT TO_GEOGRAPHY('POINT(-122.35 37.55)')");

    // Then SQLDescribeCol DataType, SQL_DESC_CONCISE_TYPE, and SQL_DESC_TYPE should be <sql_type>
    require_sql_type(stmt, 1, test_case.sql_type);

    // And SQL_DESC_TYPE_NAME should be GEOGRAPHY
    require_sql_type_name(stmt, 1, "GEOGRAPHY");
  }
}

TEST_CASE("should report geometry concise type sql_type for format when ODBC_USE_CUSTOM_SQL_DATA_TYPES is flag_state",
          "[geometry]") {
  // Given Snowflake client is logged in
  Connection conn;

  struct Case {
    const char* format;
    const char* flag_state;
    SQLSMALLINT sql_type;
  };
  const Case cases[] = {
      {"GeoJSON", "FALSE", SQL_VARCHAR},
      {"WKB", "FALSE", SQL_BINARY},
      {"GeoJSON", "TRUE", SQL_SF_OBJECT},
      {"WKB", "TRUE", SQL_BINARY},
  };

  for (const auto& test_case : cases) {
    INFO(test_case.format << " ODBC_USE_CUSTOM_SQL_DATA_TYPES=" << test_case.flag_state);

    // And Session parameter ODBC_USE_CUSTOM_SQL_DATA_TYPES is <flag_state>
    conn.execute(std::string("ALTER SESSION SET ODBC_USE_CUSTOM_SQL_DATA_TYPES = ") + test_case.flag_state);

    // And Session parameter GEOMETRY_OUTPUT_FORMAT is set to <format>
    conn.execute(std::string("ALTER SESSION SET GEOMETRY_OUTPUT_FORMAT = '") + test_case.format + "'");

    // When Query "SELECT TO_GEOMETRY('POINT(1820.12 890.56)')" is executed
    const auto stmt = conn.execute_fetch("SELECT TO_GEOMETRY('POINT(1820.12 890.56)')");

    // Then SQLDescribeCol DataType, SQL_DESC_CONCISE_TYPE, and SQL_DESC_TYPE should be <sql_type>
    require_sql_type(stmt, 1, test_case.sql_type);

    // And SQL_DESC_TYPE_NAME should be GEOMETRY
    require_sql_type_name(stmt, 1, "GEOMETRY");
  }
}
