/* Symbols the sandboxed runtime replaces so that no wasi:* import survives. */
#include <time.h>
#include "lua.h"
#include "lauxlib.h"

/* Lua seeds string hashing and math.random from time(); the plugin sandbox
 * has no clock, and a fixed seed keeps the plugin deterministic. */
time_t time(time_t *t) { if (t) *t = 0; return 0; }
clock_t clock(void) { return 0; }

/* loadfile/dofile have no file system to read from. */
int luaL_loadfilex(lua_State *L, const char *filename, const char *mode) {
    (void)mode;
    lua_pushfstring(L, "cannot open %s: no file system in the plugin sandbox",
                    filename ? filename : "stdin");
    return LUA_ERRFILE;
}

/* The renamed luaL_loadfilex still references stdio at link time, and
 * resolving fopen against wasi-libc drags in the preopen constructor and
 * with it fd_prestat_get, fd_close and proc_exit. Defining the handful of
 * stdio symbols it names keeps the archive members out of the link; the
 * function itself is unreachable and dropped by --gc-sections. */
#include <stdio.h>
FILE *fopen(const char *path, const char *mode) { (void)path; (void)mode; return NULL; }
FILE *freopen(const char *path, const char *mode, FILE *f) { (void)path; (void)mode; (void)f; return NULL; }
int getc(FILE *f) { (void)f; return EOF; }
size_t fread(void *p, size_t size, size_t n, FILE *f) { (void)p; (void)size; (void)n; (void)f; return 0; }
int ferror(FILE *f) { (void)f; return 1; }
int feof(FILE *f) { (void)f; return 1; }
int fclose(FILE *f) { (void)f; return EOF; }
