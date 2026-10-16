import { describe, it, beforeAll, beforeEach, afterAll, expect } from 'vitest';
import type {
  Connection,
  QueryStatus,
  RowStatement,
  SnowflakeError,
} from '../../types/sdk-types.js';
import { createLiveConnection } from '../utils/fixtures.js';
import {
  cancelStatementAsync,
  connectionGetQueryStatusWithBD,
  destroyConnectionAsync,
  executeAsync,
  isRunningNewDriverWithBD,
  NOT_IMPLEMENTED_IN_NEW_DRIVER,
  randomizeName,
  sendExecute,
  sleepAsync,
} from '../utils/index.js';
import {
  jsonResponse,
  loginSuccess,
  logoutSuccess,
  WiremockServer,
} from '../utils/wiremock/index.js';

const NOT_EXECUTING_CODE = '000605';
const POLL_MS = 250;
const STATUS_DEADLINE_MS = 30_000;

async function waitForStatus(
  connection: Connection,
  queryId: string,
  accept: (status: QueryStatus) => boolean,
): Promise<QueryStatus> {
  const deadline = Date.now() + STATUS_DEADLINE_MS;
  let status: QueryStatus | undefined;
  while (Date.now() < deadline) {
    status = await connectionGetQueryStatusWithBD(connection, queryId);
    if (accept(status)) {
      return status;
    }
    await sleepAsync(POLL_MS);
  }
  throw new Error(`query ${queryId} status stayed ${status ?? 'unknown'}`);
}

async function waitForRunningQueryId(connection: Connection, queryTag: string): Promise<string> {
  const deadline = Date.now() + STATUS_DEADLINE_MS;
  while (Date.now() < deadline) {
    const { rows } = await executeAsync(
      connection,
      `select query_id from table(information_schema.query_history_by_session())
       where execution_status = 'RUNNING' and query_tag = ?`,
      { binds: [queryTag] },
    );
    if (rows.length > 0) {
      return rows[0].QUERY_ID as string;
    }
    await sleepAsync(POLL_MS);
  }
  throw new Error(`query tagged ${queryTag} did not start running`);
}

describe('Query Cancellation', () => {
  let connection: Connection;

  beforeAll(async () => {
    connection = await createLiveConnection({}, false);
  });

  afterAll(async () => {
    await destroyConnectionAsync(connection);
  });

  it('cancels a running query', async () => {
    const queryTag = randomizeName('CANCEL');
    const { statement, completion } = sendExecute(
      connection,
      'select count(*) from table(generator(timeLimit => 60))',
      { parameters: { QUERY_TAG: queryTag } },
    );
    const failure = completion.then(
      () => undefined,
      (rejection: { error: SnowflakeError }) => rejection.error,
    );
    const queryId = await waitForRunningQueryId(connection, queryTag);

    await cancelStatementAsync(statement as RowStatement);

    expect(await failure).toMatchObject({
      name: 'OperationFailedError',
      code: '000604',
      sqlState: '57014',
      message: 'SQL execution canceled',
    });
    expect(
      await waitForStatus(connection, queryId, (status) => !connection.isStillRunning(status)),
    ).toBe('FAILED_WITH_ERROR');
  });

  it('does nothing when the query is already complete', async () => {
    const { statement } = await executeAsync(connection, 'select 1');
    if (isRunningNewDriverWithBD('BD#104')) {
      await cancelStatementAsync(statement);
      expect(await connectionGetQueryStatusWithBD(connection, statement.getQueryId()!)).toBe(
        'SUCCESS',
      );
      return;
    }
    await expect(cancelStatementAsync(statement)).rejects.toMatchObject({
      code: NOT_EXECUTING_CODE,
      sqlState: '01000',
    } satisfies Partial<SnowflakeError>);
  });
});

// TODO: the new driver's statement.cancel() resolves without waiting for the server abort,
// so a failed abort-request is not surfaced and this block is skipped for it.
describe.skipIf(NOT_IMPLEMENTED_IN_NEW_DRIVER)('Query Cancellation with Wiremock', () => {
  const queryId = '01baf79b-0108-1a60-0000-01110354a6ce';
  let wiremock: WiremockServer;

  beforeAll(async () => {
    wiremock = await WiremockServer.spawn();
  });

  beforeEach(async () => {
    await wiremock.reset();
    await wiremock.stub(loginSuccess());
    await wiremock.stub(logoutSuccess());
    await wiremock.stub({
      request: {
        method: 'POST',
        urlPathPattern: '/queries/v1/query-request.*',
      },
      response: jsonResponse(200, {
        success: true,
        code: '333333',
        data: { queryId },
      }),
    });
  });

  afterAll(async () => {
    await wiremock.destroy();
  });

  it('should throw when the abort request fails', async () => {
    await wiremock.stub({
      request: {
        method: 'POST',
        urlPathPattern: `/queries/${queryId}/abort-request.*`,
      },
      response: jsonResponse(403, {
        success: false,
        code: null,
        message: null,
        data: null,
      }),
    });
    const connection = await createLiveConnection(wiremock.connectionOptions);
    const { statement } = await executeAsync(connection, 'select 1', {
      asyncExec: true,
    });

    await expect(cancelStatementAsync(statement)).rejects.toMatchObject({
      name: 'RequestFailedError',
      code: 401002,
    });
  });
});
