'use client';

import { ErrorResponseSchema } from './schemas';
import { createApiError } from './errors';

const API_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:8080/api/v1';

interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE';
  body?: Record<string, any>;
  requiresAuth?: boolean;
}

export async function apiRequest<T>(
  path: string,
  options: RequestOptions & { schema: any }
): Promise<T> {
  const { method = 'GET', body, requiresAuth = true, schema } = options;

  const headers: HeadersInit = {
    'Content-Type': 'application/json',
  };

  if (requiresAuth) {
    const token = localStorage.getItem('auth_token');
    if (token) {
      headers['Authorization'] = `Bearer ${token}`;
    }
  }

  const response = await fetch(`${API_URL}${path}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  const data = await response.json() as any;

  if (!response.ok) {
    const errorData = ErrorResponseSchema.safeParse(data);
    if (errorData.success) {
      const { error } = errorData.data;
      throw createApiError(error.code, error.message, response.status);
    }
    throw createApiError('internal_error', 'An unexpected error occurred', response.status);
  }

  // Validate response with schema
  const parsed = schema.safeParse(data);
  if (!parsed.success) {
    console.error('Invalid response schema:', parsed.error);
    throw new Error('Invalid response from server');
  }

  return parsed.data as T;
}

export function getStoredToken(): string | null {
  return localStorage.getItem('auth_token');
}

export function setStoredToken(token: string): void {
  localStorage.setItem('auth_token', token);
}

export function clearStoredToken(): void {
  localStorage.removeItem('auth_token');
}
