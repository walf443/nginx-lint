/* Bridges the nginx-lint plugin world to a Lua script: `spec` and `check`
 * are implemented by calling into the script through one lazily created
 * Lua state. */
#include <stdlib.h>
#include <string.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "plugin.h"

#include "nginx_lint_lua.h"   /* nginx_lint_lua[], nginx_lint_lua_len */

/* The plugin API version, passed in by the Makefile from the SDK crate */
#ifndef API_VERSION
#error "API_VERSION must be defined (see the Makefile)"
#endif

/* The plugin script is not compiled in: nginx-lint-plugin-sdk writes it into this
 * buffer after the fact, as one more data segment of the finished module.
 * The buffer is zero-initialized, so it costs the runtime no bytes on disk,
 * only initial memory. Both symbols are exported by the linker (see the
 * Makefile), which for a data symbol means a global holding its address:
 * that is how the tool finds the buffer and, by reading the constant at
 * the second address, its capacity.
 *
 * Buffer layout, little-endian: u32 script length, u32 name length, the
 * name (used as the chunk name in error messages), then the script. */
#define SCRIPT_SLOT_MAX (1u << 20)
char nginx_lint_lua_script_slot[SCRIPT_SLOT_MAX];
const uint32_t nginx_lint_lua_script_slot_max = SCRIPT_SLOT_MAX;

static uint32_t read_u32(const char *p) {
    uint32_t v;
    memcpy(&v, p, sizeof(v));
    return v;
}

static lua_State *state;
/* Registry keys */
static const char PLUGIN_KEY = 'p';
static const char SPEC_KEY = 's';
static const char LIB_KEY = 'l';

static int lua_require(lua_State *L) {
    const char *name = luaL_checkstring(L, 1);
    if (strcmp(name, "nginx_lint") == 0) {
        lua_rawgetp(L, LUA_REGISTRYINDEX, &LIB_KEY);
        return 1;
    }
    return luaL_error(L, "module '%s' not found: the plugin sandbox has no package path", name);
}

static const luaL_Reg libs[] = {
    {LUA_GNAME, luaopen_base},
    {LUA_COLIBNAME, luaopen_coroutine},
    {LUA_TABLIBNAME, luaopen_table},
    {LUA_STRLIBNAME, luaopen_string},
    {LUA_MATHLIBNAME, luaopen_math},
    {LUA_UTF8LIBNAME, luaopen_utf8},
    {NULL, NULL},
};

/* Replaces the error object on top of the stack with a message string and
 * returns it. error() accepts any value, and lua_tostring gives NULL for a
 * table or nil; a NULL here would read as success in load_plugin and as an
 * empty message in runtime_failure. An object with a __tostring is rendered
 * through it, as the lua interpreter does, but under pcall: the callers are
 * outside any, and the metamethod is script code. The one allocation on the
 * fallback path, a short string, is not reached for the error Lua itself
 * raises when memory runs out, which is a preallocated string. */
static const char *error_message(lua_State *L) {
    if (lua_type(L, -1) == LUA_TSTRING || lua_type(L, -1) == LUA_TNUMBER) {
        return lua_tostring(L, -1);
    }
    if (luaL_getmetafield(L, -1, "__tostring") != LUA_TNIL) {
        lua_pushvalue(L, -2);
        if (lua_pcall(L, 1, 1, 0) == LUA_OK && lua_type(L, -1) == LUA_TSTRING) {
            lua_remove(L, -2);
            return lua_tostring(L, -1);
        }
        lua_pop(L, 1);  /* the failed call's error, or a non-string result */
    }
    const char *type = luaL_typename(L, -1);
    lua_pop(L, 1);
    lua_pushfstring(L, "(error object is a %s value)", type);
    return lua_tostring(L, -1);
}

/* Whether the table at index has no array part but does have fields: a
 * single record handed over where a list of them was expected. */
static int is_lone_record(lua_State *L, int index) {
    index = lua_absindex(L, index);
    if (lua_rawlen(L, index) != 0) return 0;
    lua_pushnil(L);
    if (lua_next(L, index) == 0) return 0;
    lua_pop(L, 2);
    return 1;
}

/* Runs the plugin script, keeping its table in the registry. Returns NULL on
 * success, or the error message (owned by the Lua stack). */
static const char *load_plugin(lua_State *L) {
    for (const luaL_Reg *lib = libs; lib->func; lib++) {
        luaL_requiref(L, lib->name, lib->func, 1);
        lua_pop(L, 1);
    }
    lua_pushcfunction(L, lua_require);
    lua_setglobal(L, "require");

    if (luaL_loadbufferx(L, (const char *)nginx_lint_lua, nginx_lint_lua_len, "=nginx_lint", "t") != LUA_OK
        || lua_pcall(L, 0, 1, 0) != LUA_OK) {
        return error_message(L);
    }
    lua_rawsetp(L, LUA_REGISTRYINDEX, &LIB_KEY);

    const char *slot = nginx_lint_lua_script_slot;
    uint32_t script_len = read_u32(slot);
    uint32_t name_len = read_u32(slot + 4);
    if (script_len == 0 || name_len > SCRIPT_SLOT_MAX - 8
        || script_len > SCRIPT_SLOT_MAX - 8 - name_len) {
        lua_pushstring(L, "no plugin script embedded: build this plugin with nginx-lint-plugin-sdk");
        return lua_tostring(L, -1);
    }
    /* A chunk name starting with '@' is a file name to Lua's error messages */
    char chunkname[256];
    size_t n = name_len < sizeof(chunkname) - 2 ? name_len : sizeof(chunkname) - 2;
    chunkname[0] = '@';
    memcpy(chunkname + 1, slot + 8, n);
    chunkname[1 + n] = '\0';
    if (luaL_loadbufferx(L, slot + 8 + name_len, script_len, chunkname, "t") != LUA_OK
        || lua_pcall(L, 0, 1, 0) != LUA_OK) {
        return error_message(L);
    }
    if (!lua_istable(L, -1)) {
        lua_pushstring(L, "plugin script must return a table with `spec` and `check`");
        return lua_tostring(L, -1);
    }
    lua_rawsetp(L, LUA_REGISTRYINDEX, &PLUGIN_KEY);

    /* spec may be a table or a function returning one */
    lua_rawgetp(L, LUA_REGISTRYINDEX, &PLUGIN_KEY);
    lua_getfield(L, -1, "spec");
    if (lua_isfunction(L, -1)) {
        if (lua_pcall(L, 0, 1, 0) != LUA_OK) return error_message(L);
    }
    if (!lua_istable(L, -1)) {
        lua_pushstring(L, "plugin `spec` must be a table or a function returning one");
        return lua_tostring(L, -1);
    }
    lua_rawsetp(L, LUA_REGISTRYINDEX, &SPEC_KEY);
    lua_pop(L, 1);
    return NULL;
}

static const char *load_error;

/* A failed load stays failed: every spec()/check() call would otherwise run
 * the script's top level again and leak another copy of the message. */
static lua_State *get_state(void) {
    if (state) return state;
    if (load_error) return NULL;
    state = luaL_newstate();
    if (!state) {
        load_error = "cannot create Lua state";
        return NULL;
    }
    const char *err = load_plugin(state);
    if (err) {
        load_error = strdup(err);
        if (!load_error) load_error = "not enough memory";
        lua_close(state);
        state = NULL;
        return NULL;
    }
    return state;
}

/* --- Lua -> C conversions ------------------------------------------------ */

/* Length of the well-formed UTF-8 sequence starting at s (n bytes left), or
 * 0 if there is none: Unicode's Table 3-7, which excludes overlong forms,
 * surrogates and anything past U+10FFFF, the same rules the host applies. */
static size_t utf8_sequence_len(const unsigned char *s, size_t n) {
    unsigned char c = s[0];
    if (c < 0x80) return 1;
    size_t len;
    unsigned char lo = 0x80, hi = 0xBF;
    if (c >= 0xC2 && c <= 0xDF) len = 2;
    else if (c == 0xE0) { len = 3; lo = 0xA0; }
    else if (c >= 0xE1 && c <= 0xEC) len = 3;
    else if (c == 0xED) { len = 3; hi = 0x9F; }
    else if (c == 0xEE || c == 0xEF) len = 3;
    else if (c == 0xF0) { len = 4; lo = 0x90; }
    else if (c >= 0xF1 && c <= 0xF3) len = 4;
    else if (c == 0xF4) { len = 4; hi = 0x8F; }
    else return 0;
    if (n < len || s[1] < lo || s[1] > hi) return 0;
    for (size_t i = 2; i < len; i++) {
        if (s[i] < 0x80 || s[i] > 0xBF) return 0;
    }
    return len;
}

/* Zeroed heap memory for the WIT values handed back to the host, or the
 * "not enough memory" error Lua itself raises when it runs out: a finding
 * under the exports' pcall, and outside one (runtime_failure, or a state
 * that never loaded) the panic handler and a trap, there being nothing
 * left to report with. The generated cabi_realloc aborts outright, which
 * is why the shim allocates for itself. */
static void *checked_alloc(lua_State *L, size_t size) {
    void *p = calloc(1, size ? size : 1);
    if (!p) {
        if (L) luaL_error(L, "not enough memory");
        abort();
    }
    return p;
}

/* Copies a Lua string into a WIT string. Lua strings are bytes and WIT
 * strings are UTF-8, and the host rejects the whole spec() or check()
 * result over one bad byte; a message built with string.sub on multibyte
 * text is the usual way to get one. Each ill-formed byte becomes U+FFFD
 * instead, so the finding survives and says almost what it meant. */
static void dup_utf8(lua_State *L, plugin_string_t *out, const char *s, size_t len) {
    const unsigned char *bytes = (const unsigned char *)s;
    size_t i = 0, out_len = 0;
    while (i < len) {
        size_t n = utf8_sequence_len(bytes + i, len - i);
        if (n == 0) { out_len += 3; i += 1; }
        else { out_len += n; i += n; }
    }
    unsigned char *buf = checked_alloc(L, out_len);
    if (out_len == len) {
        memcpy(buf, s, len);
        out->ptr = buf;
        out->len = len;
        return;
    }
    size_t o = 0;
    for (i = 0; i < len;) {
        size_t n = utf8_sequence_len(bytes + i, len - i);
        if (n == 0) {
            buf[o++] = 0xEF; buf[o++] = 0xBF; buf[o++] = 0xBD;
            i += 1;
        } else {
            memcpy(buf + o, bytes + i, n);
            o += n;
            i += n;
        }
    }
    out->ptr = buf;
    out->len = out_len;
}

static void string_field(lua_State *L, int index, const char *key, plugin_string_t *out) {
    lua_getfield(L, index, key);
    size_t len = 0;
    const char *s = lua_tolstring(L, -1, &len);
    dup_utf8(L, out, s ? s : "", s ? len : 0);
    lua_pop(L, 1);
}

static void option_string_field(lua_State *L, int index, const char *key, plugin_option_string_t *out) {
    lua_getfield(L, index, key);
    size_t len = 0;
    const char *s = lua_tolstring(L, -1, &len);
    out->is_some = s != NULL;
    if (s) dup_utf8(L, &out->val, s, len);
    lua_pop(L, 1);
}

/* Any number with an integral value counts: `/` yields floats in Lua, so an
 * offset computed as (a + b) / 2 is 3.0, and dropping it would silently turn
 * a range fix into a whole-line one. */
static void option_u32_field(lua_State *L, int index, const char *key, plugin_option_u32_t *out) {
    lua_getfield(L, index, key);
    int isnum = 0;
    lua_Integer v = lua_tointegerx(L, -1, &isnum);
    out->is_some = isnum && v >= 0 && v <= UINT32_MAX;
    if (out->is_some) out->val = (uint32_t)v;
    lua_pop(L, 1);
}

static bool bool_field(lua_State *L, int index, const char *key) {
    lua_getfield(L, index, key);
    bool v = lua_toboolean(L, -1);
    lua_pop(L, 1);
    return v;
}

static void spec_from_lua(lua_State *L, int index, plugin_plugin_spec_t *ret) {
    memset(ret, 0, sizeof(*ret));
    index = lua_absindex(L, index);
    string_field(L, index, "name", &ret->name);
    string_field(L, index, "category", &ret->category);
    string_field(L, index, "description", &ret->description);
    plugin_string_dup(&ret->api_version, API_VERSION);
    option_string_field(L, index, "severity", &ret->severity);
    option_string_field(L, index, "why", &ret->why);
    option_string_field(L, index, "bad_example", &ret->bad_example);
    option_string_field(L, index, "good_example", &ret->good_example);
    option_string_field(L, index, "min_nginx_version", &ret->min_nginx_version);
    option_string_field(L, index, "max_nginx_version", &ret->max_nginx_version);

    lua_getfield(L, index, "references");
    if (lua_istable(L, -1)) {
        size_t n = lua_rawlen(L, -1);
        ret->references.is_some = true;
        ret->references.val.len = n;
        ret->references.val.ptr = checked_alloc(L, n * sizeof(plugin_string_t));
        for (size_t i = 0; i < n; i++) {
            lua_rawgeti(L, -1, (lua_Integer)i + 1);
            size_t len = 0;
            const char *s = lua_tolstring(L, -1, &len);
            dup_utf8(L, &ret->references.val.ptr[i], s ? s : "", s ? len : 0);
            lua_pop(L, 1);
        }
    }
    lua_pop(L, 1);
}

static void fix_from_lua(lua_State *L, int index, nginx_lint_plugin_types_fix_t *fix) {
    memset(fix, 0, sizeof(*fix));
    index = lua_absindex(L, index);
    plugin_option_u32_t line;
    option_u32_field(L, index, "line", &line);
    fix->line = line.is_some ? line.val : 0;
    option_string_field(L, index, "old_text", &fix->old_text);
    string_field(L, index, "new_text", &fix->new_text);
    fix->delete_line = bool_field(L, index, "delete_line");
    fix->insert_after = bool_field(L, index, "insert_after");
    option_u32_field(L, index, "start_offset", &fix->start_offset);
    option_u32_field(L, index, "end_offset", &fix->end_offset);
}

/* Fills one lint-error from the table at `index`; rule and category come
 * from the spec so a script only says what and where. */
static void error_from_lua(lua_State *L, int index, plugin_lint_error_t *e) {
    memset(e, 0, sizeof(*e));
    index = lua_absindex(L, index);
    lua_rawgetp(L, LUA_REGISTRYINDEX, &SPEC_KEY);
    string_field(L, -1, "name", &e->rule);
    string_field(L, -1, "category", &e->category);
    lua_pop(L, 1);

    string_field(L, index, "message", &e->message);
    lua_getfield(L, index, "severity");
    const char *severity = lua_tostring(L, -1);
    e->severity = (severity && strcmp(severity, "error") == 0)
        ? NGINX_LINT_PLUGIN_TYPES_SEVERITY_ERROR
        : NGINX_LINT_PLUGIN_TYPES_SEVERITY_WARNING;
    lua_pop(L, 1);
    option_u32_field(L, index, "line", &e->line);
    option_u32_field(L, index, "column", &e->column);

    lua_getfield(L, index, "fixes");
    if (lua_istable(L, -1)) {
        size_t n = lua_rawlen(L, -1);
        /* `fixes = d:replace_with(...)` where `:with_fix(...)` was meant */
        if (is_lone_record(L, -1)) {
            luaL_error(L, "a finding's `fixes` must be a list of fixes; got a single fix (use :with_fix or wrap it in { })");
        }
        e->fixes.len = n;
        e->fixes.ptr = checked_alloc(L, n * sizeof(nginx_lint_plugin_types_fix_t));
        for (size_t i = 0; i < n; i++) {
            lua_rawgeti(L, -1, (lua_Integer)i + 1);
            if (!lua_istable(L, -1)) {
                luaL_error(L, "fix %d of a finding is a %s, not a table", (int)i + 1, luaL_typename(L, -1));
            }
            fix_from_lua(L, -1, &e->fixes.ptr[i]);
            lua_pop(L, 1);
        }
    }
    lua_pop(L, 1);
}

/* Fills the list at argument 1 (a light userdata) from the findings table
 * at argument 2. Runs under lua_pcall, so the shape checks are luaL_error. */
static int convert_findings(lua_State *L) {
    plugin_list_lint_error_t *ret = lua_touserdata(L, 1);
    if (lua_isnoneornil(L, 2)) return 0;
    if (!lua_istable(L, 2)) {
        return luaL_error(L, "check() must return a list of findings, not a %s", luaL_typename(L, 2));
    }
    size_t n = lua_rawlen(L, 2);
    /* `return found` where `return { found }` was meant: a single finding
     * has no array part, and would otherwise read as none. */
    if (is_lone_record(L, 2)) {
        return luaL_error(L, "check() must return a list of findings; got a single finding (wrap it in { })");
    }
    ret->len = n;
    ret->ptr = checked_alloc(L, n * sizeof(plugin_lint_error_t));
    for (size_t i = 0; i < n; i++) {
        lua_rawgeti(L, 2, (lua_Integer)i + 1);
        if (!lua_istable(L, -1)) {
            return luaL_error(L, "finding %d returned by check() is a %s, not a table",
                              (int)i + 1, luaL_typename(L, -1));
        }
        error_from_lua(L, -1, &ret->ptr[i]);
        lua_pop(L, 1);
    }
    return 0;
}

/* A single error-severity finding carrying `message`, for failures of the
 * runtime itself (a script that does not load, or throws). */
static void runtime_failure(lua_State *L, const char *message, plugin_list_lint_error_t *ret) {
    plugin_lint_error_t *e = checked_alloc(L, sizeof(*e));
    if (L) {
        lua_rawgetp(L, LUA_REGISTRYINDEX, &SPEC_KEY);
        if (lua_istable(L, -1)) {
            string_field(L, -1, "name", &e->rule);
            string_field(L, -1, "category", &e->category);
        }
        lua_pop(L, 1);
    }
    if (!e->rule.ptr) plugin_string_dup(&e->rule, "lua-plugin");
    if (!e->category.ptr) plugin_string_dup(&e->category, "plugin");
    dup_utf8(L, &e->message, message, strlen(message));
    e->severity = NGINX_LINT_PLUGIN_TYPES_SEVERITY_ERROR;
    ret->ptr = e;
    ret->len = 1;
}

/* --- C -> Lua conversions ------------------------------------------------ */

static void push_string(lua_State *L, const plugin_string_t *s) {
    lua_pushlstring(L, (const char *)s->ptr, s->len);
}

static void set_string(lua_State *L, const char *key, const plugin_string_t *s) {
    push_string(L, s);
    lua_setfield(L, -2, key);
}

static void set_option_string(lua_State *L, const char *key, const plugin_option_string_t *s) {
    if (!s->is_some) return;
    set_string(L, key, &s->val);
}

static void set_integer(lua_State *L, const char *key, lua_Integer v) {
    lua_pushinteger(L, v);
    lua_setfield(L, -2, key);
}

static void set_option_u32(lua_State *L, const char *key, const plugin_option_u32_t *v) {
    if (v->is_some) set_integer(L, key, v->val);
}

static void set_bool(lua_State *L, const char *key, bool v) {
    lua_pushboolean(L, v);
    lua_setfield(L, -2, key);
}

static const char *argument_type_name(nginx_lint_plugin_data_types_argument_type_t t) {
    switch (t) {
    case NGINX_LINT_PLUGIN_DATA_TYPES_ARGUMENT_TYPE_QUOTED_STRING: return "quoted";
    case NGINX_LINT_PLUGIN_DATA_TYPES_ARGUMENT_TYPE_SINGLE_QUOTED_STRING: return "single_quoted";
    case NGINX_LINT_PLUGIN_DATA_TYPES_ARGUMENT_TYPE_VARIABLE: return "variable";
    default: return "literal";
    }
}

static void push_directive(lua_State *L, const nginx_lint_plugin_data_types_directive_data_t *d,
                           const plugin_list_u32_t *children) {
    lua_createtable(L, 0, 24);
    lua_pushstring(L, "directive");
    lua_setfield(L, -2, "kind");
    set_string(L, "name", &d->name);

    lua_createtable(L, (int)d->args.len, 0);
    for (size_t i = 0; i < d->args.len; i++) {
        const nginx_lint_plugin_data_types_argument_info_t *a = &d->args.ptr[i];
        lua_createtable(L, 0, 7);
        set_string(L, "value", &a->value);
        set_string(L, "raw", &a->raw);
        lua_pushstring(L, argument_type_name(a->arg_type));
        lua_setfield(L, -2, "type");
        set_integer(L, "line", a->line);
        set_integer(L, "column", a->column);
        set_integer(L, "start_offset", a->start_offset);
        set_integer(L, "end_offset", a->end_offset);
        lua_rawseti(L, -2, (lua_Integer)i + 1);
    }
    lua_setfield(L, -2, "args");

    set_integer(L, "line", d->line);
    set_integer(L, "column", d->column);
    set_integer(L, "start_offset", d->start_offset);
    set_integer(L, "end_offset", d->end_offset);
    set_integer(L, "end_line", d->end_line);
    set_integer(L, "end_column", d->end_column);
    set_string(L, "leading_whitespace", &d->leading_whitespace);
    set_string(L, "trailing_whitespace", &d->trailing_whitespace);
    set_string(L, "space_before_terminator", &d->space_before_terminator);
    set_bool(L, "has_block", d->has_block);
    set_bool(L, "block_is_raw", d->block_is_raw);
    set_option_string(L, "block_raw_content", &d->block_raw_content);
    set_option_string(L, "closing_brace_leading_whitespace", &d->closing_brace_leading_whitespace);
    set_option_string(L, "block_trailing_whitespace", &d->block_trailing_whitespace);
    set_option_string(L, "trailing_comment", &d->trailing_comment_text);
    set_integer(L, "name_end_column", d->name_end_column);
    set_integer(L, "name_end_offset", d->name_end_offset);
    set_option_u32(L, "block_start_line", &d->block_start_line);
    set_option_u32(L, "block_start_column", &d->block_start_column);
    set_option_u32(L, "block_start_offset", &d->block_start_offset);

    lua_createtable(L, (int)children->len, 0);
    for (size_t i = 0; i < children->len; i++) {
        lua_pushinteger(L, (lua_Integer)children->ptr[i] + 1);
        lua_rawseti(L, -2, (lua_Integer)i + 1);
    }
    lua_setfield(L, -2, "children");
}

static void push_comment(lua_State *L, const nginx_lint_plugin_data_types_comment_info_t *c) {
    lua_createtable(L, 0, 8);
    lua_pushstring(L, "comment");
    lua_setfield(L, -2, "kind");
    set_string(L, "text", &c->text);
    set_integer(L, "line", c->line);
    set_integer(L, "column", c->column);
    set_string(L, "leading_whitespace", &c->leading_whitespace);
    set_string(L, "trailing_whitespace", &c->trailing_whitespace);
    set_integer(L, "start_offset", c->start_offset);
    set_integer(L, "end_offset", c->end_offset);
}

static void push_blank_line(lua_State *L, const nginx_lint_plugin_data_types_blank_line_info_t *b) {
    lua_createtable(L, 0, 4);
    lua_pushstring(L, "blank_line");
    lua_setfield(L, -2, "kind");
    set_integer(L, "line", b->line);
    set_string(L, "content", &b->content);
    set_integer(L, "start_offset", b->start_offset);
}

/* Pushes the config table built by nginx_lint._build_config from the host's
 * snapshot. Indices are converted to 1-based on the way in. Raises on a
 * failure, including running out of memory on a large config; the caller
 * is protected. */
static void push_config(lua_State *L, const nginx_lint_plugin_config_api_config_snapshot_t *snap,
                        const plugin_string_t *path) {
    lua_rawgetp(L, LUA_REGISTRYINDEX, &LIB_KEY);
    lua_getfield(L, -1, "_build_config");
    lua_remove(L, -2);

    lua_createtable(L, (int)snap->all_items.len, 0);
    for (size_t i = 0; i < snap->all_items.len; i++) {
        const nginx_lint_plugin_config_api_flat_item_t *item = &snap->all_items.ptr[i];
        switch (item->value.tag) {
        case NGINX_LINT_PLUGIN_PARSER_TYPES_CONFIG_ITEM_VALUE_DIRECTIVE_ITEM:
            push_directive(L, &item->value.val.directive_item, &item->child_indices);
            break;
        case NGINX_LINT_PLUGIN_PARSER_TYPES_CONFIG_ITEM_VALUE_COMMENT_ITEM:
            push_comment(L, &item->value.val.comment_item);
            break;
        default:
            push_blank_line(L, &item->value.val.blank_line_item);
            break;
        }
        lua_rawseti(L, -2, (lua_Integer)i + 1);
    }

    lua_createtable(L, (int)snap->top_level_indices.len, 0);
    for (size_t i = 0; i < snap->top_level_indices.len; i++) {
        lua_pushinteger(L, (lua_Integer)snap->top_level_indices.ptr[i] + 1);
        lua_rawseti(L, -2, (lua_Integer)i + 1);
    }

    lua_createtable(L, (int)snap->include_context.len, 0);
    for (size_t i = 0; i < snap->include_context.len; i++) {
        push_string(L, &snap->include_context.ptr[i]);
        lua_rawseti(L, -2, (lua_Integer)i + 1);
    }

    push_string(L, path);
    lua_call(L, 4, 1);
}

/* --- exports -------------------------------------------------------------- */

/* Everything an export does on the Lua side runs under one lua_pcall, in
 * these two functions. Any Lua API call that allocates can raise when
 * memory runs out — building the config table for a large file is the
 * likely place — and the shim's own allocations raise the same way (see
 * checked_alloc). Outside a pcall that is the panic handler, abort() and
 * a wasm trap, where inside it is a finding that says "not enough
 * memory". Arguments are light userdata. */

/* (spec-out) */
static int protected_spec(lua_State *L) {
    plugin_plugin_spec_t *ret = lua_touserdata(L, 1);
    lua_rawgetp(L, LUA_REGISTRYINDEX, &SPEC_KEY);
    spec_from_lua(L, -1, ret);
    return 0;
}

/* (snapshot, path, findings-out): builds the config, calls check() and
 * converts what it returns. Shape errors are luaL_error, so a finding or
 * fix that is not a table reads as a message rather than a trap. */
static int protected_check(lua_State *L) {
    const nginx_lint_plugin_config_api_config_snapshot_t *snap = lua_touserdata(L, 1);
    const plugin_string_t *path = lua_touserdata(L, 2);
    plugin_list_lint_error_t *ret = lua_touserdata(L, 3);

    lua_rawgetp(L, LUA_REGISTRYINDEX, &PLUGIN_KEY);
    lua_getfield(L, -1, "check");
    lua_remove(L, -2);
    push_config(L, snap, path);
    push_string(L, path);
    lua_call(L, 2, 1);

    lua_pushcfunction(L, convert_findings);
    lua_pushlightuserdata(L, ret);
    lua_pushvalue(L, -3);
    lua_call(L, 2, 0);
    return 0;
}

void exports_plugin_spec(plugin_plugin_spec_t *ret) {
    memset(ret, 0, sizeof(*ret));
    lua_State *L = get_state();
    const char *failure = load_error;
    if (L) {
        int top = lua_gettop(L);
        lua_pushcfunction(L, protected_spec);
        lua_pushlightuserdata(L, ret);
        if (lua_pcall(L, 1, 0, 0) != LUA_OK) {
            plugin_plugin_spec_free(ret);
            memset(ret, 0, sizeof(*ret));
            failure = error_message(L);
        }
        if (!failure) {
            lua_settop(L, top);
            return;
        }
        /* The message is owned by the stack; copy it before unwinding */
        dup_utf8(L, &ret->description, failure, strlen(failure));
        lua_settop(L, top);
    } else {
        dup_utf8(NULL, &ret->description, failure, strlen(failure));
    }
    /* Surface the failure through the spec so the host shows it */
    plugin_string_dup(&ret->name, "lua-plugin");
    plugin_string_dup(&ret->category, "plugin");
    plugin_string_dup(&ret->api_version, API_VERSION);
}

void exports_plugin_check(plugin_borrow_config_t cfg, plugin_string_t *path, plugin_list_lint_error_t *ret) {
    ret->ptr = NULL;
    ret->len = 0;
    lua_State *L = get_state();
    if (!L) {
        nginx_lint_plugin_config_api_config_drop_borrow(cfg);
        runtime_failure(NULL, load_error, ret);
        return;
    }
    int top = lua_gettop(L);

    nginx_lint_plugin_config_api_config_snapshot_t snap;
    nginx_lint_plugin_config_api_method_config_snapshot(cfg, &snap);
    nginx_lint_plugin_config_api_config_drop_borrow(cfg);

    lua_pushcfunction(L, protected_check);
    lua_pushlightuserdata(L, &snap);
    lua_pushlightuserdata(L, path);
    lua_pushlightuserdata(L, ret);
    int status = lua_pcall(L, 3, 0, 0);
    nginx_lint_plugin_config_api_config_snapshot_free(&snap);
    if (status != LUA_OK) {
        plugin_list_lint_error_free(ret);
        runtime_failure(L, error_message(L), ret);
    }
    lua_settop(L, top);
}
