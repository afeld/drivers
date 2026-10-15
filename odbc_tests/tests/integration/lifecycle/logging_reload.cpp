#include <sql.h>
#include <sqlext.h>

#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>

#include <catch2/catch_test_macros.hpp>

#include "Connection.hpp"
#include "EnvOverride.hpp"
#include "HandleWrapper.hpp"
#include "WiremockClient.hpp"
#include "compatibility.hpp"
#include "odbc_cast.hpp"
#include "odbc_matchers.hpp"
#include "put_get_utils.hpp"
#include "test_setup.hpp"

namespace fs = std::filesystem;
using pg_utils::TempTestDir;

static void write_ini(const fs::path& ini_path, const std::string& body) {
  {
    std::ofstream ini(ini_path, std::ios::trunc);
    REQUIRE(ini.good());
    // Under iODBC the driver requires DriverManagerEncoding=UTF-32 to match the
    // DM's 4-byte SQLWCHARs; without it the first wide-string call mis-decodes.
    // SF_ODBC_INI bypasses the other lookup-chain candidates, so the encoding
    // entry has to live in the per-test INI itself.
    if (is_iodbc_test_suite()) {
      ini << "DriverManagerEncoding=UTF-32\n";
    }
    ini << body;
  }
  // The driver rejects an sf.odbc.ini that is group- or world-writable. 0600
  // keeps the file owner-only regardless of the runner's umask, matching the
  // posture a real deployment uses.
  fs::permissions(ini_path, fs::perms::owner_read | fs::perms::owner_write, fs::perm_options::replace);
}

static std::string read_all_files_in(const fs::path& dir) {
  std::ostringstream combined;
  if (!fs::exists(dir)) return {};
  for (const auto& entry : fs::directory_iterator(dir)) {
    if (entry.is_regular_file()) {
      std::ifstream in(entry.path(), std::ios::binary);
      combined << in.rdbuf();
    }
  }
  return combined.str();
}

// Allocates and connects a DBC under `env`, handing the open handle to a
// ConnectedConnectionWrapper before the connect assertion so a failed connect
// disconnects and frees rather than leaking the handle.
static ConnectedConnectionWrapper connect_keep_open(const std::string& conn_str, SQLHENV env) {
  SQLHDBC dbc = SQL_NULL_HDBC;
  REQUIRE(SQLAllocHandle(SQL_HANDLE_DBC, env, &dbc) == SQL_SUCCESS);
  ConnectedConnectionWrapper wrapper(dbc);

  SQLRETURN ret =
      SQLDriverConnect(dbc, nullptr, sqlchar(conn_str.c_str()), SQL_NTS, nullptr, 0, nullptr, SQL_DRIVER_NOPROMPT);
  REQUIRE_THAT(OdbcResult(ret, SQL_HANDLE_DBC, dbc), OdbcMatchers::Succeeded());
  return wrapper;
}

TEST_CASE("sf.odbc.ini logging reload: editing LogPath takes effect on the next connect",
          "[lifecycle][logging][reload]") {
  SKIP_OLD_DRIVER("", "Old driver does not reload sf.odbc.ini logging on connect");
#ifdef _WIN32
  SKIP("WireMock tests not yet validated on Windows");
#endif

  // The INI lives in its own directory so that scanning the log directory only
  // ever picks up driver log files, never the ini. Both directories clean up in
  // their destructors on every path, including a mid-test REQUIRE throw.
  TempTestDir ini_dir("odbc_reload_ini_");
  TempTestDir log_dir("odbc_reload_log_");
  auto ini_path = ini_dir.path() / "sf.odbc.ini";
  auto log_file = log_dir.path() / "sf_driver.log";

  // First connect uses an ini with logging enabled but no LogPath, so no file
  // sink is installed.
  write_ini(ini_path, "LogLevel=INFO\n");
  EnvOverride env_override("SF_ODBC_INI", ini_path.string());

  WiremockClient wm;
  wm.add_mapping_file("auth/login_success_any.json");
  wm.add_mapping_file("session/logout_success.json");
  std::string conn_str = get_wiremock_connection_string(wm);

  EnvironmentHandleWrapper env;
  REQUIRE(SQLSetEnvAttr(env.getHandle(), SQL_ATTR_ODBC_VERSION, reinterpret_cast<SQLPOINTER>(SQL_OV_ODBC3), 0) ==
          SQL_SUCCESS);

  // The first connection stays open for the whole test. The driver manager
  // keeps the driver loaded while any connection is live, so OdbcGlobals and
  // the fingerprint fast-path state persist across the second connect — the
  // reload happens without a process restart or a fresh driver load.
  ConnectedConnectionWrapper dbc1 = connect_keep_open(conn_str, env.getHandle());
  CHECK_FALSE(fs::exists(log_file));

  // Rewriting the same ini in place with a LogPath and a higher verbosity
  // changes the fingerprint (size and mtime), so the next connect re-reads it
  // and installs the file sink.
  write_ini(ini_path, "LogLevel=DEBUG\nLogPath=" + log_dir.path().string() + "\n");

  ConnectedConnectionWrapper dbc2 = connect_keep_open(conn_str, env.getHandle());

  CHECK(fs::exists(log_file));
  std::string contents = read_all_files_in(log_dir.path());
  CHECK(contents.find("ODBC logging configuration applied") != std::string::npos);
  CHECK(contents.find("connect_with_params") != std::string::npos);
}

TEST_CASE("sf.odbc.ini logging reload: an ini created after the first connect installs a sink on the next connect",
          "[lifecycle][logging][reload]") {
  SKIP_OLD_DRIVER("", "Old driver does not reload sf.odbc.ini logging on connect");
#ifdef _WIN32
  SKIP("WireMock tests not yet validated on Windows");
#endif
  // The first connect runs with no ini on disk, so DriverManagerEncoding is
  // never negotiated; under iODBC that mis-decodes the first wide-string call
  // for reasons unrelated to the file-appears transition this case exercises.
  if (is_iodbc_test_suite()) {
    SKIP("absent SF_ODBC_INI leaves DriverManagerEncoding unset under iODBC");
  }

  // The INI lives in its own directory so scanning the log directory only ever
  // picks up driver log files. Both directories clean up in their destructors.
  TempTestDir ini_dir("odbc_reload_absent_ini_");
  TempTestDir log_dir("odbc_reload_absent_log_");
  auto ini_path = ini_dir.path() / "sf.odbc.ini";
  auto log_file = log_dir.path() / "sf_driver.log";

  // SF_ODBC_INI points at a path that does not exist yet. Discovery skips the
  // missing candidate, so the first connect installs no file sink.
  REQUIRE_FALSE(fs::exists(ini_path));
  EnvOverride env_override("SF_ODBC_INI", ini_path.string());

  WiremockClient wm;
  wm.add_mapping_file("auth/login_success_any.json");
  wm.add_mapping_file("session/logout_success.json");
  std::string conn_str = get_wiremock_connection_string(wm);

  EnvironmentHandleWrapper env;
  REQUIRE(SQLSetEnvAttr(env.getHandle(), SQL_ATTR_ODBC_VERSION, reinterpret_cast<SQLPOINTER>(SQL_OV_ODBC3), 0) ==
          SQL_SUCCESS);

  ConnectedConnectionWrapper dbc1 = connect_keep_open(conn_str, env.getHandle());
  CHECK_FALSE(fs::exists(log_file));

  // Creating the ini at the previously-missing path flips the winning candidate
  // from "none" to this file, so the fingerprint changes and the next connect
  // re-reads it and installs the file sink while the first connection stays open.
  write_ini(ini_path, "LogLevel=DEBUG\nLogPath=" + log_dir.path().string() + "\n");

  ConnectedConnectionWrapper dbc2 = connect_keep_open(conn_str, env.getHandle());

  CHECK(fs::exists(log_file));
  std::string contents = read_all_files_in(log_dir.path());
  CHECK(contents.find("ODBC logging configuration applied") != std::string::npos);
  CHECK(contents.find("connect_with_params") != std::string::npos);
}

TEST_CASE("sf.odbc.ini discovery: SF_ODBC_INI pointing at a directory does not error the connect",
          "[lifecycle][logging][reload]") {
  SKIP_OLD_DRIVER("", "Old driver does not consult SF_ODBC_INI for logging discovery");
#ifdef _WIN32
  SKIP("WireMock tests not yet validated on Windows");
#endif
  // A directory holds no DriverManagerEncoding entry, so under iODBC the first
  // wide-string call mis-decodes for reasons unrelated to the discovery
  // fall-through this case exercises.
  if (is_iodbc_test_suite()) {
    SKIP("directory SF_ODBC_INI leaves DriverManagerEncoding unset under iODBC");
  }

  TempTestDir dir("odbc_reload_dir_ini_");
  EnvOverride env_override("SF_ODBC_INI", dir.path().string());

  WiremockClient wm;
  wm.add_mapping_file("auth/login_success_any.json");
  wm.add_mapping_file("session/logout_success.json");
  std::string conn_str = get_wiremock_connection_string(wm);

  // A directory is not a regular file, so discovery skips the SF_ODBC_INI
  // candidate and falls through to the remaining paths instead of erroring the
  // connect (SNOW-4243048).
  EnvironmentHandleWrapper env;
  REQUIRE(SQLSetEnvAttr(env.getHandle(), SQL_ATTR_ODBC_VERSION, reinterpret_cast<SQLPOINTER>(SQL_OV_ODBC3), 0) ==
          SQL_SUCCESS);

  ConnectedConnectionWrapper dbc = connect_keep_open(conn_str, env.getHandle());
}
