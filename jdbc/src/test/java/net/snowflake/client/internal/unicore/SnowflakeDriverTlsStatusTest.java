package net.snowflake.client.internal.unicore;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.io.File;
import java.sql.SQLException;
import net.snowflake.client.api.driver.FipsStatus;
import net.snowflake.client.api.driver.SnowflakeDriver;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.Test;

public class SnowflakeDriverTlsStatusTest {
  @Test
  public void shouldReportWhetherThisProcessIsTheFipsDriverWithoutConnection() throws SQLException {
    String corePath = System.getenv("CORE_PATH");
    Assumptions.assumeTrue(
        corePath != null && new File(corePath).exists(),
        "jdbc_bridge native library not built; skipping JNI FIPS status probe");

    boolean expectedFips = Boolean.parseBoolean(System.getenv("JDBC_EXPECT_FIPS_BUILD"));
    FipsStatus status = new SnowflakeDriver().getFipsStatus();

    assertEquals(expectedFips, status.isFips());
  }
}
