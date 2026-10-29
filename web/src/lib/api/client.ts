import type { ZodType } from 'zod';
import { ErrorResponseSchema, UserSchema } from './schemas';
import { createApiError, type ApiError } from './errors';

export const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'http://localhost:8080/api/v1';

const TOKEN_KEY = 'satsbridge.token';

export function getStoredToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

function storeToken(token: string): void {
  try {
    localStorage.setItem(TOKEN_KEY, token);
  } catch {
    // Storage blocked (private mode): the session lives in memory until reload.
  }
}

export function clearStoredToken(): void {
  try {
    localStorage.removeItem(TOKEN_KEY);
  } catch {
    // Nothing to clear when storage is unavailable.
  }
}

async function readJson(res: Response): Promise<unknown> {
  const text = await res.text();
  if (!text) return null;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return null;
  }
}

async function send(path: string, init: RequestInit, token?: string): Promise<Response> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  try {
    return await fetch(`${API_URL}${path}`, { ...init, headers });
  } catch {
    throw createApiError('offline', 0);
  }
}

function errorFrom(status: number, data: unknown): ApiError {
  const parsed = ErrorResponseSchema.safeParse(data);
  if (parsed.success) {
    return createApiError(parsed.data.error.code, status, parsed.data.error.message);
  }
  return createApiError('internal_error', status);
}

let sessionCreation: Promise<string> | null = null;

// Creates an anonymous user once; concurrent callers share the same request.
export function ensureSession(): Promise<string> {
  const existing = getStoredToken();
  if (existing) return Promise.resolve(existing);
  if (!sessionCreation) {
    sessionCreation = (async () => {
      const res = await send('/users', { method: 'POST', body: '{}' });
      const data = await readJson(res);
      if (!res.ok) throw errorFrom(res.status, data);
      const user = UserSchema.safeParse(data);
      if (!user.success) throw createApiError('invalid_response', res.status);
      storeToken(user.data.token);
      return user.data.token;
    })().finally(() => {
      sessionCreation = null;
    });
  }
  return sessionCreation;
}

export interface RequestSpec<T> {
  method: 'GET' | 'POST';
  path: string;
  body?: unknown;
  auth: boolean;
  schema: ZodType<T>;
}

export async function apiRequest<T>(spec: RequestSpec<T>): Promise<T> {
  return attempt(spec, false);
}

async function attempt<T>(spec: RequestSpec<T>, retried: boolean): Promise<T> {
  const init: RequestInit = {
    method: spec.method,
    body: spec.body === undefined ? undefined : JSON.stringify(spec.body),
  };
  const token = spec.auth ? await ensureSession() : undefined;
  const res = await send(spec.path, init, token);
  const data = await readJson(res);

  if (res.status === 401 && spec.auth && !retried) {
    clearStoredToken();
    await ensureSession();
    return attempt(spec, true);
  }
  if (!res.ok) throw errorFrom(res.status, data);

  const parsed = spec.schema.safeParse(data);
  if (!parsed.success) {
    throw createApiError('invalid_response', res.status);
  }
  return parsed.data;
}
