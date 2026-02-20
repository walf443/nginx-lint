/**
 * Shared include-context helpers for Config implementations.
 *
 * Both config-builder.ts (real parser output) and test-helpers.ts (mocks)
 * need identical include-context logic. This module provides a single
 * implementation to avoid duplication.
 */

export interface IncludeContextMethods {
  includeContext(): string[];
  isIncludedFrom(context: string): boolean;
  isIncludedFromHttp(): boolean;
  isIncludedFromHttpServer(): boolean;
  isIncludedFromHttpLocation(): boolean;
  isIncludedFromStream(): boolean;
  immediateParentContext(): string | undefined;
}

export function makeIncludeContextMethods(inclCtx: string[]): IncludeContextMethods {
  return {
    includeContext() { return inclCtx; },
    isIncludedFrom(context: string) { return inclCtx.includes(context); },
    isIncludedFromHttp() { return inclCtx.includes("http"); },
    isIncludedFromHttpServer() {
      const httpIndex = inclCtx.indexOf("http");
      const serverIndex = inclCtx.indexOf("server");
      return httpIndex !== -1 && serverIndex !== -1 && httpIndex < serverIndex;
    },
    isIncludedFromHttpLocation() {
      const httpIndex = inclCtx.indexOf("http");
      const locationIndex = inclCtx.indexOf("location");
      return httpIndex !== -1 && locationIndex !== -1 && httpIndex < locationIndex;
    },
    isIncludedFromStream() { return inclCtx.includes("stream"); },
    immediateParentContext() { return inclCtx.length > 0 ? inclCtx[inclCtx.length - 1] : undefined; },
  };
}
