import assert from 'assert';
import { WIP_ConnectionConfig, WIP_ConnectionOptions } from '../../../../lib/connection/types';
import OriginalAuthWorkloadIdentity from '../../../../lib/authentication/auth_workload_identity';
import ConnectionConfig from '../../../../lib/connection/connection_config';

describe('Workload Identity Authentication', async () => {
  let AuthWorkloadIdentity: typeof OriginalAuthWorkloadIdentity;

  function getConnectionConfig(
    options: Omit<WIP_ConnectionOptions, 'account'> = {},
  ): WIP_ConnectionConfig {
    return new ConnectionConfig({
      authenticator: 'WORKLOAD_IDENTITY',
      account: 'test-account',
      ...options,
    });
  }

  before(() => {
    AuthWorkloadIdentity = require('../../../../lib/authentication/auth_workload_identity').default;
  });

  describe('authenticate() with AZURE', () => {
    const connectionConfig = getConnectionConfig({
      workloadIdentityProvider: 'AZURE',
    });

    it('throws error when impersonation path is provided', async () => {
      const auth = new AuthWorkloadIdentity({
        ...connectionConfig,
        workloadIdentityImpersonationPath: ['test-path'],
      });
      await assert.rejects(
        auth.authenticate(),
        /workloadIdentityImpersonationPath for AZURE is not supported/,
      );
    });
  });
});
