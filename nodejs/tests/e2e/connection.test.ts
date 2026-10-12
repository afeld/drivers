import { describe, it, expect } from 'vitest';
import { createConnection } from './utils/fixtures.js';
import {
  createTestConnection,
  destroyConnectionAsync,
  isRunningNewDriverWithBD,
  snowflake,
} from './utils/index.js';

describe('Connection', () => {
  it('.connect() connects on valid parameters', async () => {
    const connection = createTestConnection();
    try {
      await new Promise<void>((resolve, reject) => {
        connection.connect((err) => (err ? reject(err) : resolve()));
      });
      expect(connection.isUp()).toBe(true);
    } finally {
      await destroyConnectionAsync(connection);
    }
  });

  it('should surface a rejected login through the .connect() callback', async () => {
    const connection = createConnection({
      username: 'incorrect-username',
    });
    const error = await new Promise<Error | undefined>((resolve) => {
      connection.connect((err) => resolve(err));
    });
    expect(error).toMatchObject({
      name: 'OperationFailedError',
      code: '390144',
      message: expect.stringContaining('JWT token is invalid'),
    });
  });

  it('.connectAsync() connects on valid parameters', async () => {
    const connection = createTestConnection();
    try {
      await connection.connectAsync();
      expect(connection.isUp()).toBe(true);
    } finally {
      await destroyConnectionAsync(connection);
    }
  });
  it('should surface a rejected login from .connectAsync()', async () => {
    const connection = createConnection({
      username: 'incorrect-username',
    });
    const expectedError = {
      name: 'OperationFailedError',
      code: '390144',
      message: expect.stringContaining('JWT token is invalid'),
    };
    if (isRunningNewDriverWithBD('BD#11')) {
      await expect(connection.connectAsync()).rejects.toMatchObject(expectedError);
    } else {
      const error = await new Promise<Error | undefined>((resolve) => {
        void connection.connectAsync((err) => resolve(err));
      });
      expect(error).toMatchObject(expectedError);
    }
  });

  it('should reject a connection without a user name', async () => {
    const options = { account: 'a', password: 'p' };
    if (isRunningNewDriverWithBD('BD#101')) {
      const connection = snowflake.createConnection(options);
      await expect(connection.connectAsync()).rejects.toMatchObject({
        name: 'InvalidParameterError',
        code: undefined,
        message:
          "Configuration error: Configuration validation failed (1 issue(s)): user: Missing required parameter 'user'",
      });
    } else {
      expect(() => snowflake.createConnection(options)).toThrow(
        expect.objectContaining({
          name: 'MissingParameterError',
          code: 404003,
          message: 'A user name must be specified.',
        }),
      );
    }
  });

  it('destroys the connection and transitions to a disconnected state', async () => {
    const connection = createTestConnection();
    await connection.connectAsync();
    await destroyConnectionAsync(connection);
    expect(connection.isUp()).toBe(false);
  });

  describe('isValidAsync', () => {
    it('should report a connection as valid after connect', async () => {
      const connection = createTestConnection();
      try {
        await connection.connectAsync();
        await expect(connection.isValidAsync()).resolves.toBe(true);
      } finally {
        await destroyConnectionAsync(connection);
      }
    });

    it('should report a connection as invalid after destroy', async () => {
      const connection = createTestConnection();
      await connection.connectAsync();
      await destroyConnectionAsync(connection);
      await expect(connection.isValidAsync()).resolves.toBe(false);
    });
  });
});
