import { expect, it } from 'vitest';
import { getFipsStatus, type FipsStatus } from '../../src/index.js';

it('should report whether this process is the FIPS driver without a connection', () => {
  const fipsMode = process.env.SF_CORE_EXPECT_FIPS?.toLowerCase();
  const expectedFips = fipsMode === '1' || fipsMode === 'true';
  const status: FipsStatus = getFipsStatus();

  expect(status).toEqual({
    isFips: expectedFips,
  });
});
