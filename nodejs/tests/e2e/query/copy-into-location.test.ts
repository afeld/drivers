import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import type { Connection } from '../../types/sdk-types.js';
import {
  createLiveConnection,
  createTemporaryStage,
  createTemporaryTable,
} from '../utils/fixtures.js';
import { destroyConnectionAsync, executeAsync, randomizeName } from '../utils/index.js';

describe('tests/definitions/shared/query/copy_into_location.feature', () => {
  let connection: Connection;

  beforeAll(async () => {
    connection = await createLiveConnection({}, false);
  });

  afterAll(async () => {
    await destroyConnectionAsync(connection);
  });

  it('should return detailed output rows and -1 for COPY INTO stage', async () => {
    // Given Snowflake client is logged in
    void connection;

    // And a temporary stage and a table with 3 rows exist
    const stage = await createTemporaryStage(connection);
    const table = await createTemporaryTable(connection, 'id INT');
    await executeAsync(connection, `INSERT INTO ${table} VALUES (1), (2), (3)`);

    // When the table is unloaded into one file with DETAILED_OUTPUT = TRUE
    const { statement, rows } = await executeAsync(
      connection,
      `COPY INTO @${stage} FROM ${table} ` +
        'FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE ' +
        'OVERWRITE = TRUE SINGLE = TRUE',
    );

    // Then the columns are FILE_NAME, FILE_SIZE, and ROW_COUNT
    expect(statement.getColumns()!.map((column) => column.getName())).toEqual([
      'FILE_NAME',
      'FILE_SIZE',
      'ROW_COUNT',
    ]);

    // And one row is returned with ROW_COUNT 3
    expect(rows).toHaveLength(1);
    expect(typeof rows[0].FILE_NAME).toBe('string');
    expect(rows[0].FILE_NAME).not.toBe('');
    expect(typeof rows[0].ROW_COUNT).toBe('number');
    expect(rows[0].ROW_COUNT).toBe(3);

    // And getNumUpdatedRows returns -1
    expect(statement.getNumUpdatedRows()).toBe(-1);
  });

  it('should return one row per partition file and -1 for partitioned COPY INTO stage', async () => {
    // Given Snowflake client is logged in
    void connection;

    // And a temporary stage and a table with 3 rows in partition a and 4 rows in partition b exist
    const stage = await createTemporaryStage(connection);
    const table = await createTemporaryTable(connection, 'grp VARCHAR, id INT');
    await executeAsync(
      connection,
      `INSERT INTO ${table} VALUES ('a', 1), ('a', 2), ('a', 3), ('b', 4), ('b', 5), ('b', 6), ('b', 7)`,
    );

    // When the table is unloaded with PARTITION BY and DETAILED_OUTPUT = TRUE
    const { statement, rows } = await executeAsync(
      connection,
      `COPY INTO @${stage} FROM ${table} ` +
        "PARTITION BY ('p=' || grp) " +
        'FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE',
    );

    // Then the columns are PARTITION_NAME, FILE_NAME, FILE_SIZE, and ROW_COUNT
    expect(statement.getColumns()!.map((column) => column.getName())).toEqual([
      'PARTITION_NAME',
      'FILE_NAME',
      'FILE_SIZE',
      'ROW_COUNT',
    ]);

    // And each row names its partition and file
    for (const row of rows) {
      expect(typeof row.PARTITION_NAME).toBe('string');
      expect(row.PARTITION_NAME).not.toBe('');
      expect(typeof row.FILE_NAME).toBe('string');
      expect(row.FILE_NAME).not.toBe('');
    }

    // And the ROW_COUNT values are 3 and 4
    expect(rows).toHaveLength(2);
    const rowCounts = rows.map((row) => {
      expect(typeof row.ROW_COUNT).toBe('number');
      return row.ROW_COUNT;
    });
    expect([...rowCounts].sort()).toEqual([3, 4]);

    // And getNumUpdatedRows returns -1
    expect(statement.getNumUpdatedRows()).toBe(-1);
  });

  it('should return no file rows and -1 when unloading an empty table', async () => {
    // Given Snowflake client is logged in
    void connection;

    // And a temporary stage and an empty table exist
    const stage = await createTemporaryStage(connection);
    const table = await createTemporaryTable(connection, 'id INT');

    // When the empty table is unloaded into one file with DETAILED_OUTPUT = TRUE
    const { statement, rows } = await executeAsync(
      connection,
      `COPY INTO @${stage} FROM ${table} ` +
        'FILE_FORMAT = (TYPE = CSV) DETAILED_OUTPUT = TRUE HEADER = FALSE ' +
        'OVERWRITE = TRUE SINGLE = TRUE',
    );

    // Then the columns are FILE_NAME, FILE_SIZE, and ROW_COUNT
    expect(statement.getColumns()!.map((column) => column.getName())).toEqual([
      'FILE_NAME',
      'FILE_SIZE',
      'ROW_COUNT',
    ]);

    // And no file rows are returned
    expect(rows).toHaveLength(0);

    // And getNumUpdatedRows returns -1
    expect(statement.getNumUpdatedRows()).toBe(-1);
  });

  it('should fail when COPY INTO stage does not exist', async () => {
    // Given Snowflake client is logged in
    void connection;

    // And a table with one row exists and the stage does not
    const stage = randomizeName('missing_unload_stage_').toUpperCase();
    const table = await createTemporaryTable(connection, 'id INT');
    await executeAsync(connection, `INSERT INTO ${table} VALUES (1)`);

    // When the table is unloaded into the missing stage
    const execution = executeAsync(
      connection,
      `COPY INTO @${stage} FROM ${table} FILE_FORMAT = (TYPE = CSV)`,
    );

    // Then the unload fails because the stage does not exist
    await expect(execution).rejects.toMatchObject({
      error: {
        name: 'OperationFailedError',
        code: '002003',
        sqlState: '02000',
        message: expect.stringMatching(
          new RegExp(`does not exist.*${stage}|${stage}.*does not exist`, 'i'),
        ),
      },
    });
  });
});
