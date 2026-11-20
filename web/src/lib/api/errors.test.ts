import { describe, expect, it } from 'vitest';
import { createApiError, getErrorMessage } from './errors';

describe('API error mapping', () => {
  it('maps every backend error code to a human message', () => {
    const codes = [
      'validation_error',
      'not_found',
      'conflict',
      'quote_expired',
      'unauthorized',
      'rate_limited',
      'invalid_recipient',
      'recipient_not_found',
      'provider_unavailable',
      'provider_rejected',
      'internal_error',
    ];
    for (const code of codes) {
      const message = getErrorMessage(code);
      expect(message.length).toBeGreaterThan(10);
      expect(message).not.toBe('Something went wrong. Please try again.');
    }
  });

  it('keeps the code, status and server detail on the error', () => {
    const error = createApiError('validation_error', 400, 'amount is too small to cover fees');
    expect(error.code).toBe('validation_error');
    expect(error.statusCode).toBe(400);
    expect(error.detail).toBe('amount is too small to cover fees');
    expect(error.message).toBe(getErrorMessage('validation_error'));
  });

  it('falls back to a generic message for unknown codes', () => {
    expect(getErrorMessage('something_new')).toBe('Something went wrong. Please try again.');
  });
});
