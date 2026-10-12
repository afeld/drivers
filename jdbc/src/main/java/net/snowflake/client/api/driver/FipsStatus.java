package net.snowflake.client.api.driver;

/** Whether this process is the FIPS driver. */
public final class FipsStatus {
  private final boolean isFips;

  FipsStatus(boolean isFips) {
    this.isFips = isFips;
  }

  /** True when this binary was built with {@code fips} and its TLS provider is in FIPS mode. */
  public boolean isFips() {
    return isFips;
  }
}
