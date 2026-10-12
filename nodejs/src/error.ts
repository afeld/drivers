export interface SnowflakeError extends Error {
  code?: string | number;
  sqlState?: string;
  isFatal?: boolean;
}

export function invalidParameterError(message: string, code?: number): SnowflakeError {
  return Object.assign(new Error(message), { name: 'InvalidParameterError', code });
}
