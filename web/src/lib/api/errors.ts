export class ApiError extends Error {
  readonly code: string;
  readonly statusCode: number;
  // The server's own message. Shown only where it is specific and safe (e.g. refund invoice amount).
  readonly detail: string | null;

  constructor(code: string, message: string, statusCode: number, detail: string | null = null) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
    this.statusCode = statusCode;
    this.detail = detail;
  }
}

// Human messages keyed by the backend error codes plus client-side codes.
// The server's own message is never shown, so the copy stays consistent.
const MESSAGES: Record<string, string> = {
  validation_error: 'Please check the details and try again.',
  not_found: 'We could not find that. It may have been removed.',
  conflict: 'That action conflicts with the current state of this payment.',
  quote_expired: 'The exchange rate has expired. We have refreshed it for you.',
  unauthorized: 'Your session has ended. Please try again.',
  rate_limited: 'Slow down for a moment, then try again.',
  invalid_recipient: 'The recipient details are not valid. Please check them.',
  recipient_not_found: 'We could not verify this account. Please check the details.',
  provider_unavailable: 'The payout partner is temporarily unavailable. Please try again shortly.',
  provider_rejected: 'The payout partner rejected this request. Please check the details or try again.',
  invalid_signature: 'The request could not be verified.',
  internal_error: 'Something went wrong on our side. Please try again.',
  offline: 'You appear to be offline. Check your connection and try again.',
  invalid_response: 'The server sent a response we could not read. Please try again.',
};

export function getErrorMessage(code: string): string {
  return MESSAGES[code] ?? 'Something went wrong. Please try again.';
}

export function createApiError(code: string, statusCode: number, detail: string | null = null): ApiError {
  return new ApiError(code, getErrorMessage(code), statusCode, detail);
}
