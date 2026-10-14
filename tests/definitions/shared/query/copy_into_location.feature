@nodejs
Feature: COPY INTO location

  @nodejs_e2e
  Scenario: should return detailed output rows and -1 for COPY INTO stage
    Given Snowflake client is logged in
    And a temporary stage and a table with 3 rows exist
    When the table is unloaded into one file with DETAILED_OUTPUT = TRUE
    Then the columns are FILE_NAME, FILE_SIZE, and ROW_COUNT
    And one row is returned with ROW_COUNT 3
    And getNumUpdatedRows returns -1

  @nodejs_e2e
  Scenario: should return one row per partition file and -1 for partitioned COPY INTO stage
    Given Snowflake client is logged in
    And a temporary stage and a table with 3 rows in partition a and 4 rows in partition b exist
    When the table is unloaded with PARTITION BY and DETAILED_OUTPUT = TRUE
    Then the columns are PARTITION_NAME, FILE_NAME, FILE_SIZE, and ROW_COUNT
    And each row names its partition and file
    And the ROW_COUNT values are 3 and 4
    And getNumUpdatedRows returns -1

  @nodejs_e2e
  Scenario: should return no file rows and -1 when unloading an empty table
    Given Snowflake client is logged in
    And a temporary stage and an empty table exist
    When the empty table is unloaded into one file with DETAILED_OUTPUT = TRUE
    Then the columns are FILE_NAME, FILE_SIZE, and ROW_COUNT
    And no file rows are returned
    And getNumUpdatedRows returns -1

  @nodejs_e2e
  Scenario: should fail when COPY INTO stage does not exist
    Given Snowflake client is logged in
    And a table with one row exists and the stage does not
    When the table is unloaded into the missing stage
    Then the unload fails because the stage does not exist
