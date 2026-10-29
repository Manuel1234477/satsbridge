export class ApiError extends Error {
  constructor(
    public code: string,
    public message: string,
    public statusCode: number
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

const errorMessages: Record<string, string> = {
  validation_error: 'Please check your input and try again.',
  not_found: 'This item was not found. It may have been deleted.',
  conflict: 'This action conflicts with an existing operation.',
  quote_expired: 'The exchange rate has expired. Please get a new quote.',
  unauthorized: 'You are not authorized. Please try signing in again.',
  rate_limited: 'Too many requests. Please slow down and try again in a moment.',
  invalid_recipient: 'The recipient details are invalid. Please check and try again.',
  recipient_not_found: 'The recipient could not be found. Please verify the details.',
  provider_unavailable: 'The payment provider is temporarily unavailable. Please try again later.',
  provider_rejected: 'The payment provider rejected this request. Please try again or contact support.',
  invalid_signature: 'Invalid request signature.',
  internal_error: 'An unexpected error occurred. Please try again or contact support.',
};

export function getErrorMessage(code: string): string {
  return errorMessages[code] || 'An unexpected error occurred.';
}

export function createApiError(code: string, message: string, statusCode: number): ApiError {
  const friendlyMessage = getErrorMessage(code);
  return new ApiError(code, friendlyMessage || message, statusCode);
}
