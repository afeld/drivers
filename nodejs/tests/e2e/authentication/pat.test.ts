import { beforeAll, describe, expect, it } from 'vitest';
import type { Connection } from '../../types/sdk-types.js';
import getTestParameter from '../utils/getTestParameter.js';
import {
  baseConnectionOptions,
  connectAsyncWithErrorBD,
  isRunningNewDriverWithBD,
  snowflake,
} from '../utils/index.js';
import { verifySimpleQuery } from '../utils/query.js';
import { destroyConnectionAfterTest, expectConnectionNotUp, TERMINATED_QUERY } from './utils.js';

const INVALID_PAT_MESSAGE = 'Programmatic access token is invalid';

let PAT_SECRET: string;

beforeAll(() => {
  PAT_SECRET = getTestParameter('SNOWFLAKE_TEST_PAT', true);
});

function createPatConnection(overrides: Record<string, unknown>): Connection {
  return snowflake.createConnection({
    ...baseConnectionOptions,
    ...overrides,
  });
}

describe('tests/definitions/shared/authentication/PAT.feature', () => {
  it('should authenticate using PAT as password', async () => {
    // Given Authentication is set to password and valid PAT token is provided
    const connection = createPatConnection({
      password: PAT_SECRET,
    });

    // When Trying to Connect
    await connection.connectAsync();
    destroyConnectionAfterTest(connection);

    // Then Login is successful and simple query can be executed
    expect(connection.isUp()).toBe(true);
    await verifySimpleQuery(connection);
  });

  it('should authenticate using PAT as token', async () => {
    // Given Authentication is set to Programmatic Access Token and valid PAT token is provided
    const connection = createPatConnection({
      authenticator: 'PROGRAMMATIC_ACCESS_TOKEN',
      token: PAT_SECRET,
    });

    // When Trying to Connect
    await connection.connectAsync();
    destroyConnectionAfterTest(connection);

    // Then Login is successful and simple query can be executed
    expect(connection.isUp()).toBe(true);
    await verifySimpleQuery(connection);
  });

  it('should authenticate using PAT as token with lowercase authenticator', async () => {
    // Given Authentication is set to lowercase programmatic_access_token and valid PAT token is provided
    const connection = createPatConnection({
      authenticator: 'programmatic_access_token',
      token: PAT_SECRET,
    });

    // When Trying to Connect
    await connection.connectAsync();
    destroyConnectionAfterTest(connection);

    // Then Login is successful and simple query can be executed
    expect(connection.isUp()).toBe(true);
    await verifySimpleQuery(connection);
  });

  it('should fail PAT authentication when invalid token provided', async () => {
    // Given Authentication is set to Programmatic Access Token and invalid PAT token is provided
    const connection = createPatConnection({
      authenticator: 'PROGRAMMATIC_ACCESS_TOKEN',
      token: 'invalidToken',
    });

    // When Trying to Connect
    await expect(connectAsyncWithErrorBD(connection)).rejects.toMatchObject({
      name: 'OperationFailedError',
      code: '394400',
      message: expect.stringContaining(INVALID_PAT_MESSAGE),
    });

    // Then There is error returned
    await expectConnectionNotUp(connection, TERMINATED_QUERY);
  });

  it('should authenticate using PAT as token without user', async () => {
    // Given Authentication is set to Programmatic Access Token and valid PAT token is provided
    const { username, ...withoutUser } = baseConnectionOptions;
    void username;
    const connection = snowflake.createConnection({
      ...withoutUser,
      authenticator: 'PROGRAMMATIC_ACCESS_TOKEN',
      token: PAT_SECRET,
    });

    // When Trying to Connect without user
    await connection.connectAsync();
    destroyConnectionAfterTest(connection);

    // Then Login is successful and simple query can be executed
    expect(connection.isUp()).toBe(true);
    await verifySimpleQuery(connection);
  });

  it('should fail PAT authentication when the username does not match the token', async () => {
    // Given Authentication is set to Programmatic Access Token, a valid PAT is provided, and the username is not the token owner
    const connection = createPatConnection({
      authenticator: 'PROGRAMMATIC_ACCESS_TOKEN',
      token: PAT_SECRET,
      username: 'differentUsername',
    });

    // When Trying to Connect
    await expect(connectAsyncWithErrorBD(connection)).rejects.toMatchObject({
      name: 'OperationFailedError',
      code: '394400',
      message: expect.stringContaining(INVALID_PAT_MESSAGE),
    });

    // Then There is error returned
    await expectConnectionNotUp(connection, TERMINATED_QUERY);
  });
});

describe('PAT authenticator with the secret only in password', () => {
  it('connects on the old driver and rejects a missing token on the new driver', async () => {
    const connection = createPatConnection({
      authenticator: 'PROGRAMMATIC_ACCESS_TOKEN',
      password: PAT_SECRET,
    });

    if (isRunningNewDriverWithBD('BD#111')) {
      await expect(connectAsyncWithErrorBD(connection)).rejects.toMatchObject({
        name: 'InvalidParameterError',
        message: expect.stringContaining("Missing required parameter 'token'"),
      });
    } else {
      await connection.connectAsync();
      destroyConnectionAfterTest(connection);
      expect(connection.isUp()).toBe(true);
      await verifySimpleQuery(connection);
    }
  });
});
