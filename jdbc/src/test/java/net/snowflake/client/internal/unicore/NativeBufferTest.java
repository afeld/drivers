package net.snowflake.client.internal.unicore;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import net.snowflake.client.internal.api.implementation.exception.SFSQLException;
import org.junit.jupiter.api.Test;

public class NativeBufferTest {

  @Test
  public void shouldThrowCarrierWhenAddressIsReadAfterClose() {
    try (NativeBuffer buffer = NativeBuffer.copyOf(new byte[] {1, 2, 3})) {
      assertNotEquals(0L, buffer.address());
      buffer.close();
      SFSQLException error = assertThrows(SFSQLException.class, buffer::address);
      assertEquals("Native buffer is closed", error.getMessage());
    }
  }
}
