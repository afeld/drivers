package net.snowflake.client.api.statement;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.UUID;
import net.snowflake.jdbc.utils.SnowflakeIntegrationTestBase;
import org.junit.jupiter.api.Test;

/** COPY INTO location (UNLOAD) result and update-count behavior. */
public class SnowflakeUnloadTest extends SnowflakeIntegrationTestBase {

  @Test
  public void shouldReportNoResultSetAndZeroUpdateCountForCopyIntoStage() throws Exception {
    Connection conn = getDefaultConnection();
    UnloadSource source = createStageAndTable(conn, "unload_src_", "id INT", "(1), (2), (3)");

    try (Statement stmt = conn.createStatement()) {
      boolean hasResultSet = stmt.execute(detailedUnload(source.stageName, source.tableName));

      assertFalse(hasResultSet, "Unload should not expose a ResultSet");
      assertNull(stmt.getResultSet(), "Unload should not expose a ResultSet");
      assertEquals(0, stmt.getUpdateCount(), "Unload should report zero affected rows");
    }
  }

  @Test
  public void shouldReturnDetailedOutputRowsWhenExecuteQueryCopiesIntoStage() throws Exception {
    Connection conn = getDefaultConnection();
    UnloadSource source = createStageAndTable(conn, "unload_src_", "id INT", "(1), (2), (3)");

    try (Statement stmt = conn.createStatement();
        ResultSet rs = stmt.executeQuery(detailedUnload(source.stageName, source.tableName))) {
      ResultSetMetaData metadata = rs.getMetaData();
      assertEquals(3, metadata.getColumnCount());
      assertEquals("FILE_NAME", metadata.getColumnName(1));
      assertEquals("FILE_SIZE", metadata.getColumnName(2));
      assertEquals("ROW_COUNT", metadata.getColumnName(3));

      assertTrue(rs.next(), "Expected one file row");
      String fileName = rs.getString(1);
      assertFalse(rs.wasNull());
      assertFalse(fileName.isEmpty(), "FILE_NAME should be non-empty");
      long rowCount = rs.getLong(3);
      assertFalse(rs.wasNull());
      assertEquals(3L, rowCount, "ROW_COUNT should equal the source row count");
      assertFalse(rs.next(), "Expected exactly one file row");
    }
  }

  @Test
  public void shouldReturnPartitionRowsAndNoUpdateCountForPartitionedCopyIntoStage()
      throws Exception {
    Connection conn = getDefaultConnection();
    UnloadSource source =
        createStageAndTable(
            conn,
            "unload_part_",
            "grp VARCHAR, id INT",
            "('a', 1), ('a', 2), ('a', 3), ('b', 4), ('b', 5), ('b', 6), ('b', 7)");

    try (Statement stmt = conn.createStatement()) {
      boolean hasResultSet =
          stmt.execute(
              "COPY INTO @"
                  + source.stageName
                  + " FROM "
                  + source.tableName
                  + " PARTITION BY ('p=' || grp)"
                  + " FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE");

      assertTrue(hasResultSet, "Partitioned unload should expose a ResultSet");
      assertEquals(
          -1, stmt.getUpdateCount(), "Partitioned unload should not expose an update count");
      try (ResultSet rs = stmt.getResultSet()) {
        assertNotNull(rs, "Partitioned unload should expose a ResultSet");
        ResultSetMetaData metadata = rs.getMetaData();
        assertEquals(4, metadata.getColumnCount());
        assertEquals("PARTITION_NAME", metadata.getColumnName(1));
        assertEquals("FILE_NAME", metadata.getColumnName(2));
        assertEquals("FILE_SIZE", metadata.getColumnName(3));
        assertEquals("ROW_COUNT", metadata.getColumnName(4));

        List<Long> rowCounts = new ArrayList<>();
        while (rs.next()) {
          String partitionName = rs.getString(1);
          assertFalse(rs.wasNull());
          assertFalse(partitionName.isEmpty(), "PARTITION_NAME should be non-empty");
          String fileName = rs.getString(2);
          assertFalse(rs.wasNull());
          assertFalse(fileName.isEmpty(), "FILE_NAME should be non-empty");
          rowCounts.add(rs.getLong(4));
          assertFalse(rs.wasNull());
        }
        Collections.sort(rowCounts);
        assertEquals(Arrays.asList(3L, 4L), rowCounts);
      }
    }
  }

  @Test
  public void shouldReportZeroUpdateCountWhenUnloadingAnEmptyTable() throws Exception {
    Connection conn = getDefaultConnection();
    UnloadSource source = createStageAndTable(conn, "unload_empty_", "id INT", null);

    try (Statement stmt = conn.createStatement()) {
      boolean hasResultSet = stmt.execute(detailedUnload(source.stageName, source.tableName));

      assertFalse(hasResultSet, "Unloading an empty table should not expose a ResultSet");
      assertNull(stmt.getResultSet(), "Unloading an empty table should not expose a ResultSet");
      assertEquals(
          0, stmt.getUpdateCount(), "Unloading an empty table should report zero affected rows");
    }
  }

  @Test
  public void shouldReturnNoFileRowsWhenExecuteQueryUnloadsAnEmptyTable() throws Exception {
    Connection conn = getDefaultConnection();
    UnloadSource source = createStageAndTable(conn, "unload_empty_", "id INT", null);

    try (Statement stmt = conn.createStatement();
        ResultSet rs = stmt.executeQuery(detailedUnload(source.stageName, source.tableName))) {
      ResultSetMetaData metadata = rs.getMetaData();
      assertEquals(3, metadata.getColumnCount());
      assertEquals("FILE_NAME", metadata.getColumnName(1));
      assertEquals("FILE_SIZE", metadata.getColumnName(2));
      assertEquals("ROW_COUNT", metadata.getColumnName(3));
      assertFalse(rs.next(), "An empty table should write no files");
    }
  }

  @Test
  public void shouldThrowWhenCopyIntoStageDoesNotExist() throws Exception {
    Connection conn = getDefaultConnection();
    String stageName = temporaryStageName();
    String tableName = createTempTable(conn, "unload_src_", "id INT");
    execute(conn, "INSERT INTO " + tableName + " VALUES (1)");

    try (Statement stmt = conn.createStatement()) {
      SQLException exception =
          assertThrows(
              SQLException.class,
              () -> stmt.execute("COPY INTO @" + stageName + " FROM " + tableName));
      assertEquals("02000", exception.getSQLState());
      assertEquals(2003, exception.getErrorCode());
      assertNotNull(exception.getMessage());
      String message = exception.getMessage().toUpperCase();
      assertTrue(
          message.contains(stageName.toUpperCase()), "Missing stage error should name the stage");
      assertTrue(
          message.contains("DOES NOT EXIST"),
          "Missing stage should fail with the server object-not-found message");
    }
  }

  private UnloadSource createStageAndTable(
      Connection conn, String prefix, String columns, String values) throws Exception {
    String stageName = temporaryStageName();
    String tableName = createTempTable(conn, prefix, columns);
    execute(conn, "CREATE OR REPLACE TEMPORARY STAGE " + stageName);
    if (values != null) {
      execute(conn, "INSERT INTO " + tableName + " VALUES " + values);
    }
    return new UnloadSource(stageName, tableName);
  }

  private static String detailedUnload(String stageName, String tableName) {
    return "COPY INTO @"
        + stageName
        + " FROM "
        + tableName
        + " FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE"
        + " OVERWRITE = TRUE SINGLE = TRUE";
  }

  private static String temporaryStageName() {
    return "unload_stage_" + UUID.randomUUID().toString().replace("-", "");
  }

  private static final class UnloadSource {
    private final String stageName;
    private final String tableName;

    private UnloadSource(String stageName, String tableName) {
      this.stageName = stageName;
      this.tableName = tableName;
    }
  }
}
