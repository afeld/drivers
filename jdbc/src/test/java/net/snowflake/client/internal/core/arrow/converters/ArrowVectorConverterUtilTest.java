package net.snowflake.client.internal.core.arrow.converters;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.Map;
import net.snowflake.client.api.resultset.SnowflakeType;
import net.snowflake.client.internal.api.implementation.exception.SFSQLException;
import net.snowflake.client.internal.api.implementation.parameters.FrozenParametersRegistry;
import net.snowflake.client.internal.api.implementation.parameters.Parameter;
import net.snowflake.client.internal.unicore.ConfigSettingFactory;
import net.snowflake.client.internal.unicore.protobuf_gen.DatabaseDriverV1.ConfigSetting;
import org.apache.arrow.memory.BufferAllocator;
import org.apache.arrow.memory.RootAllocator;
import org.apache.arrow.vector.BigIntVector;
import org.apache.arrow.vector.VarCharVector;
import org.apache.arrow.vector.VectorSchemaRoot;
import org.apache.arrow.vector.complex.StructVector;
import org.apache.arrow.vector.types.Types;
import org.apache.arrow.vector.types.pojo.ArrowType;
import org.apache.arrow.vector.types.pojo.Field;
import org.apache.arrow.vector.types.pojo.FieldType;
import org.apache.arrow.vector.types.pojo.Schema;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;
import org.junit.jupiter.params.provider.ValueSource;

class ArrowVectorConverterUtilTest extends BaseConverterTest {

  private final BufferAllocator allocator = new RootAllocator(Long.MAX_VALUE);

  @AfterEach
  void closeAllocator() {
    allocator.close();
  }

  @ParameterizedTest
  @CsvSource({
    "timestamp_ntz, TIMESTAMP_NTZ",
    "TIMESTAMP_NTZ, TIMESTAMP_NTZ",
    "timestamp_ltz, TIMESTAMP_LTZ",
    "TIMESTAMP_LTZ, TIMESTAMP_LTZ",
    "timestamp_tz, TIMESTAMP_TZ",
    "TIMESTAMP_TZ, TIMESTAMP_TZ"
  })
  void shouldResolveLogicalTypeMetadataCaseInsensitively(String logicalType, String expected) {
    Map<String, String> metadata = Collections.singletonMap("logicalType", logicalType);
    Field field =
        new Field("col", new FieldType(true, ArrowType.Utf8.INSTANCE, null, metadata), null);

    assertEquals(
        SnowflakeType.valueOf(expected),
        ArrowVectorConverterUtil.getSnowflakeTypeFromFieldMetadata(field));
  }

  @Test
  void shouldReturnNullWhenLogicalTypeMetadataIsAbsent() {
    Field field =
        new Field(
            "col",
            new FieldType(true, ArrowType.Utf8.INSTANCE, null, Collections.emptyMap()),
            null);

    assertNull(ArrowVectorConverterUtil.getSnowflakeTypeFromFieldMetadata(field));
  }

  @Test
  void shouldDispatchBareTimestampArrowColumnToNtzConverterWhenSessionMappingIsLowercaseWireName() {
    DataConversionContext context = contextWithTimestampMapping("timestamp_ntz");

    try (BigIntVector vector = createBareTimestampVector()) {
      ArrowVectorConverter converter = ArrowVectorConverterUtil.initConverter(vector, context, 0);
      assertInstanceOf(BigIntToTimestampNTZConverter.class, converter);
    }
  }

  @Test
  void shouldDispatchBareTimestampArrowColumnToLtzConverterWhenSessionMappingIsLowercaseWireName() {
    DataConversionContext context = contextWithTimestampMapping("timestamp_ltz");

    try (BigIntVector vector = createBareTimestampVector()) {
      ArrowVectorConverter converter = ArrowVectorConverterUtil.initConverter(vector, context, 0);
      assertInstanceOf(BigIntToTimestampLTZConverter.class, converter);
    }
  }

  @Test
  void shouldDispatchTwoFieldBareTimestampToLtzConverterWhenSessionMappingIsLowercaseWireName() {
    DataConversionContext context = contextWithTimestampMapping("timestamp_ltz");

    try (VectorSchemaRoot root = createBareTimestampStruct("epoch", "fraction")) {
      StructVector vector = (StructVector) root.getVector("col");
      ArrowVectorConverter converter = ArrowVectorConverterUtil.initConverter(vector, context, 0);
      assertInstanceOf(TwoFieldStructToTimestampLTZConverter.class, converter);
    }
  }

  @Test
  void shouldDispatchTwoFieldBareTimestampToNtzConverterWhenSessionMappingIsLowercaseWireName() {
    DataConversionContext context = contextWithTimestampMapping("timestamp_ntz");

    try (VectorSchemaRoot root = createBareTimestampStruct("epoch", "fraction")) {
      StructVector vector = (StructVector) root.getVector("col");
      ArrowVectorConverter converter = ArrowVectorConverterUtil.initConverter(vector, context, 0);
      assertInstanceOf(TwoFieldStructToTimestampNTZConverter.class, converter);
    }
  }

  @Test
  void shouldRejectBareTimestampArrowColumnWhenSessionMappingIsNeitherNtzNorLtz() {
    DataConversionContext context = contextWithTimestampMapping("not_a_timestamp");

    try (BigIntVector vector = createBareTimestampVector()) {
      SFSQLException ex =
          assertThrows(
              SFSQLException.class,
              () -> ArrowVectorConverterUtil.initConverter(vector, context, 0));
      assertEquals(
          "Unsupported TIMESTAMP mapping for bare TIMESTAMP: not_a_timestamp", ex.getMessage());
    }
  }

  @ParameterizedTest
  @ValueSource(strings = {"text", "variant"})
  void shouldParseLowercaseArrowLogicalTypeNames(String logicalType) {
    Map<String, String> meta = new HashMap<>();
    meta.put("logicalType", logicalType);
    FieldType fieldType = new FieldType(true, Types.MinorType.VARCHAR.getType(), null, meta);
    try (VarCharVector vector = new VarCharVector("col", fieldType, allocator)) {
      vector.setSafe(0, "x".getBytes(StandardCharsets.UTF_8));
      vector.setValueCount(1);
      ArrowVectorConverter converter = ArrowVectorConverterUtil.initConverter(vector, this, 0);
      assertInstanceOf(VarCharConverter.class, converter);
    }
  }

  private static DataConversionContext contextWithTimestampMapping(String mapping) {
    Map<String, ConfigSetting> parameters = new HashMap<>();
    parameters.put(
        Parameter.CLIENT_TIMESTAMP_TYPE_MAPPING.getKey(), ConfigSettingFactory.from(mapping));
    return SessionDataConversionContext.from(new FrozenParametersRegistry(parameters));
  }

  private BigIntVector createBareTimestampVector() {
    Map<String, String> metadata = new HashMap<>();
    metadata.put("logicalType", "TIMESTAMP");
    metadata.put("scale", "0");
    FieldType fieldType = new FieldType(true, Types.MinorType.BIGINT.getType(), null, metadata);
    BigIntVector vector = new BigIntVector("col_ts", fieldType, allocator);
    vector.setSafe(0, 0L);
    vector.setValueCount(1);
    return vector;
  }

  private VectorSchemaRoot createBareTimestampStruct(String firstChild, String secondChild) {
    Map<String, String> metadata = new HashMap<>();
    metadata.put("logicalType", "TIMESTAMP");
    metadata.put("scale", "0");
    Field structField =
        new Field(
            "col",
            new FieldType(true, ArrowType.Struct.INSTANCE, null, metadata),
            Arrays.asList(signedIntField(firstChild, 64), signedIntField(secondChild, 32)));
    VectorSchemaRoot root =
        VectorSchemaRoot.create(new Schema(Collections.singletonList(structField)), allocator);
    root.allocateNew();
    return root;
  }

  private static Field signedIntField(String name, int bitWidth) {
    return new Field(
        name, new FieldType(true, new ArrowType.Int(bitWidth, true), null, null), null);
  }
}
