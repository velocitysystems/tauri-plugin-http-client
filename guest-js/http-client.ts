import { invoke } from '@tauri-apps/api/core';
import { HttpHeaders } from './headers';
import { HttpClientError, HttpErrorCode, parseError } from './errors';
import type { BodyEncoding, RequestOptions, HttpResponse, RawFetchRequest, FetchResponseMetadata } from './types';

let requestCounter = 0;

// Generates a unique ID used as both a tracking key in Rust's InFlightRequests
// map and a cancellation token for the abort_request IPC command. The counter
// handles rapid calls within the same millisecond.
function generateRequestId(): string {
   requestCounter += 1;
   return `req-${Date.now()}-${requestCounter}`;
}

/**
 * Sends an HTTP request through the Tauri HTTP client plugin.
 *
 * All URL validation and security checks happen in the Rust backend.
 *
 * @param url - The URL to request
 * @param options - Optional request configuration
 * @returns A response object with status, headers, and body accessors
 * @throws {HttpClientError} On network errors, security violations, or abort
 */
export async function request(url: string, options?: RequestOptions): Promise<HttpResponse> {
   let requestId: string | undefined,
       abortHandler: (() => void) | undefined;

   if (options?.signal) {
      requestId = generateRequestId();

      if (options.signal.aborted) {
         throw parseError({ code: 'ABORTED', message: 'request aborted' });
      }

      abortHandler = (): void => {
         // Fire-and-forget abort to Rust
         invoke('plugin:http-client|abort_request', { requestId }).catch(() => {
            // Ignore errors from abort (request may have already completed)
         });
      };

      options.signal.addEventListener('abort', abortHandler, { once: true });
   }

   try {
      const payload = buildPayload(url, requestId, options),
            ipcResult: ArrayBuffer | number[] = await invoke('plugin:http-client|fetch', { request: payload });

      return wrapResponse(decodeIpcResult(ipcResult));
   } catch(err: unknown) {
      throw parseError(err);
   } finally {
      if (abortHandler && options?.signal) {
         options.signal.removeEventListener('abort', abortHandler);
      }
   }
}

function buildPayload(url: string, requestId: string | undefined, options?: RequestOptions): RawFetchRequest {
   const payload: RawFetchRequest = { url };

   if (options?.method) {
      payload.method = options.method;
   }

   if (options?.headers) {
      if (options.headers instanceof HttpHeaders) {
         payload.headers = options.headers.toRecord();
      } else {
         payload.headers = options.headers;
      }
   }

   if (options?.body !== undefined) {
      const encoded: { body: string; encoding: BodyEncoding } = encodeBody(options.body);

      payload.body = encoded.body;
      payload.bodyEncoding = encoded.encoding;
   }

   if (options?.timeout !== undefined) {
      payload.timeoutMs = options.timeout;
   }

   if (requestId) {
      payload.requestId = requestId;
   }

   if (options?.maxRetries !== undefined) {
      payload.maxRetries = options.maxRetries;
   }

   return payload;
}

function encodeBody(body: string | Uint8Array | Record<string, unknown>): { body: string; encoding: BodyEncoding } {
   if (typeof body === 'string') {
      return { body, encoding: 'utf8' };
   }

   if (body instanceof Uint8Array) {
      return { body: uint8ArrayToBase64(body), encoding: 'base64' };
   }

   return { body: JSON.stringify(body), encoding: 'utf8' };
}

// Manual loop + btoa/atob for broad WebView compatibility (avoids relying
// on Uint8Array.toBase64 which is not available in all runtimes).
function uint8ArrayToBase64(bytes: Uint8Array): string {
   let binary = '';

   for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
   }

   return btoa(binary);
}

/**
 * Normalized shape produced by `decodeIpcResult` — metadata plus raw body bytes.
 * This is the internal representation used by `wrapResponse`.
 */
interface DecodedResponse {
   metadata: FetchResponseMetadata;
   body: Uint8Array;
}

/**
 * Decodes the raw value returned by `invoke('plugin:http-client|fetch')` into
 * a unified `DecodedResponse`.
 *
 * The binary frame format is:
 * `[4-byte BE metadata length][metadata JSON bytes][body bytes]`
 *
 * Tauri delivers `InvokeResponseBody::Raw` as an `ArrayBuffer` on most
 * platforms. On some platforms (notably Android), raw bytes may arrive as a
 * `number[]` instead; this function normalizes both forms before decoding.
 */
export function decodeIpcResult(result: ArrayBuffer | number[]): DecodedResponse {
   // Tauri may deliver raw bytes as a number[] instead of ArrayBuffer on some
   // platforms. Normalize to ArrayBuffer before decoding the binary frame.
   const buf: ArrayBuffer = result instanceof ArrayBuffer
      ? result
      : new Uint8Array(result as number[]).buffer;

   return decodeBinaryFrame(buf);
}

function isValidFetchMetadata(val: unknown): val is FetchResponseMetadata {
   if (val === null || typeof val !== 'object') {
      return false;
   }

   const o = val as Record<string, unknown>;

   return typeof o.status === 'number'
      && typeof o.statusText === 'string'
      && typeof o.headers === 'object' && o.headers !== null
      && typeof o.url === 'string'
      && typeof o.redirected === 'boolean'
      && typeof o.retryCount === 'number';
}

/**
 * Decodes the binary frame format used on desktop platforms:
 * `[4-byte BE metadata length][metadata JSON bytes][body bytes]`
 *
 * Throws `HttpClientError` with code `PROTOCOL_ERROR` if the frame is
 * malformed (truncated, invalid UTF-8 metadata, invalid JSON, or missing
 * required fields).
 */
function decodeBinaryFrame(buf: ArrayBuffer): DecodedResponse {
   if (buf.byteLength < 4) {
      throw new HttpClientError(HttpErrorCode.PROTOCOL_ERROR, `binary frame too small: ${buf.byteLength} bytes`);
   }

   const view = new DataView(buf),
         metaLen = view.getUint32(0);

   if (metaLen === 0) {
      throw new HttpClientError(HttpErrorCode.PROTOCOL_ERROR, 'binary frame metadata length is 0');
   }

   if (4 + metaLen > buf.byteLength) {
      throw new HttpClientError(
         HttpErrorCode.PROTOCOL_ERROR,
         `binary frame metadata length ${metaLen} exceeds buffer size ${buf.byteLength}`
      );
   }

   const metaBytes = new Uint8Array(buf, 4, metaLen);

   let metaJson: string;

   try {
      metaJson = new TextDecoder('utf-8', { fatal: true }).decode(metaBytes);
   } catch{
      throw new HttpClientError(HttpErrorCode.PROTOCOL_ERROR, 'binary frame metadata is not valid UTF-8');
   }

   let parsed: unknown;

   try {
      parsed = JSON.parse(metaJson);
   } catch{
      throw new HttpClientError(HttpErrorCode.PROTOCOL_ERROR, 'binary frame metadata is not valid JSON');
   }

   if (!isValidFetchMetadata(parsed)) {
      throw new HttpClientError(HttpErrorCode.PROTOCOL_ERROR, 'binary frame metadata is missing required fields');
   }

   const metadata = parsed as FetchResponseMetadata,
         body = new Uint8Array(buf, 4 + metaLen);

   return { metadata, body };
}

function wrapResponse(decoded: DecodedResponse): HttpResponse {
   const { metadata, body } = decoded,
         headers = new HttpHeaders(metadata.headers);

   // Cache decoded body values
   let textValue: string | undefined,
       bytesValue: Uint8Array | undefined;

   return {
      status: metadata.status,
      statusText: metadata.statusText,
      headers,
      url: metadata.url,
      redirected: metadata.redirected,
      ok: metadata.status >= 200 && metadata.status < 300, // mirrors fetch() Response.ok
      retryCount: metadata.retryCount,

      text(): string {
         if (textValue === undefined) {
            textValue = new TextDecoder().decode(body);
         }

         return textValue;
      },

      json<T = unknown>(): T {
         return JSON.parse(this.text()) as T;
      },

      bytes(): Uint8Array {
         if (bytesValue === undefined) {
            // Slice to own a non-shared copy (body is a view into the IPC buffer)
            bytesValue = body.slice();
         }

         return bytesValue;
      },
   };
}
