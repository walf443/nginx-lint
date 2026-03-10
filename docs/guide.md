# nginx-lint — Getting Started Guide

nginx configuration file linter with 30+ built-in rules covering
security, best practices, style, syntax, and deprecation.


## Installation

### GitHub Releases (pre-built binaries)

Download from https://github.com/walf443/nginx-lint/releases

```bash
# Example: Linux x86_64
curl -LO https://github.com/walf443/nginx-lint/releases/latest/download/nginx-lint-x86_64-unknown-linux-gnu.tar.gz
tar xzf nginx-lint-x86_64-unknown-linux-gnu.tar.gz
sudo mv nginx-lint /usr/local/bin/
```

Available targets:
- `x86_64-unknown-linux-gnu` (Linux x86_64)
- `aarch64-unknown-linux-gnu` (Linux ARM64)
- `aarch64-apple-darwin` (macOS Apple Silicon)

### Docker

```bash
docker run --rm -v /etc/nginx:/etc/nginx:ro \
  ghcr.io/walf443/nginx-lint:latest /etc/nginx/nginx.conf
```

### Build from source

```bash
git clone https://github.com/walf443/nginx-lint.git
cd nginx-lint
cargo install --path .
```


## Basic Usage

```bash
# Lint a configuration file
nginx-lint /etc/nginx/nginx.conf

# Lint multiple files
nginx-lint /etc/nginx/conf.d/*.conf

# Automatically fix problems
nginx-lint --fix /etc/nginx/nginx.conf

# JSON output (for scripting/CI)
nginx-lint -o json /etc/nginx/nginx.conf

# GitHub Actions annotation format
nginx-lint -o github-actions /etc/nginx/nginx.conf

# Only fail on errors, not warnings
nginx-lint --no-fail-on-warnings /etc/nginx/nginx.conf
```


## Rules

```bash
# List all available rules
nginx-lint why --list

# Show detailed documentation for a specific rule
nginx-lint why server-tokens-enabled
```

Rules are grouped into categories:

| Category | Examples |
|----------|----------|
| security | server_tokens, autoindex, SSL/TLS settings |
| best-practices | proxy settings, gzip, error_log, etc. |
| style | indentation, trailing whitespace |
| syntax | missing semicolons, unmatched braces, etc. |
| deprecation | deprecated directives (ssl on, listen http2) |


## Configuration (.nginx-lint.toml)

Generate a default configuration file:

```bash
nginx-lint config init
```

Example `.nginx-lint.toml`:

```toml
[color]
ui = "auto"       # "auto", "always", or "never"
error = "red"
warning = "yellow"

# Disable a specific rule
[rules.gzip-not-enabled]
enabled = false

# Configure indentation
[rules.indent]
indent_size = 4   # or "auto" to detect from file

# Allow specific SSL/TLS protocols
[rules.deprecated-ssl-protocol]
allowed_protocols = ["TLSv1.2", "TLSv1.3"]

# Support non-standard directives from extension modules
[rules.invalid-directive-context]
additional_contexts = { server = ["rtmp"], upstream = ["rtmp"] }

# Custom block directives for parser
[parser]
block_directives = ["rtmp", "application"]
```

Validate your configuration:

```bash
nginx-lint config validate
```

View the full configuration reference:

```bash
# Human-readable Markdown format
nginx-lint config schema --format markdown

# JSON Schema (for editors and tools)
nginx-lint config schema
```


## Suppressing Warnings (Ignore Comments)

Both a rule name and a reason are required.

Comment on the line before:

```nginx
# nginx-lint:ignore server-tokens-enabled required by monitoring system
server_tokens on;
```

Inline comment:

```nginx
server_tokens on; # nginx-lint:ignore server-tokens-enabled required by monitoring
```

Multiple rules:

```nginx
# nginx-lint:ignore server-tokens-enabled,autoindex-enabled legacy config
server_tokens on;
```


## Partial Config Files (Context)

When linting files that are included into a parent config (e.g.,
sites-available snippets), specify the parent context so that
context-aware rules work correctly:

```bash
# Via command line
nginx-lint --context http,server /etc/nginx/sites-available/mysite.conf
```

```nginx
# Via comment in the file
# nginx-lint:context http,server
location /api {
    proxy_pass http://backend;
}
```


## Include Resolution

nginx-lint automatically follows `include` directives. Both absolute
paths and glob patterns are supported.

For path mapping (e.g., sites-enabled -> sites-available), add to `.nginx-lint.toml`:

```toml
[[include.path_map]]
from = "sites-enabled"
to   = "sites-available"
```

Set a prefix for resolving relative include paths:

```bash
nginx-lint -p /etc/nginx /etc/nginx/nginx.conf
```

Or in `.nginx-lint.toml`:

```toml
[include]
prefix = "/etc/nginx"
```


## CI Integration

### GitHub Actions (recommended)

```yaml
- uses: walf443/nginx-lint-action@v0
  with:
    files: /etc/nginx/nginx.conf
```

### GitHub Actions (manual)

```yaml
- name: Lint nginx config
  run: nginx-lint --format github-actions /etc/nginx/nginx.conf
```

### Docker in CI

```yaml
- name: Lint nginx config
  run: |
    docker run --rm \
      -v ${{ github.workspace }}:/workspace:ro \
      ghcr.io/walf443/nginx-lint:latest \
      /workspace/nginx.conf
```


## More Information

| Command | Description |
|---------|-------------|
| `nginx-lint --help` | Show CLI options |
| `nginx-lint why --list` | List all rules |
| `nginx-lint why <rule-name>` | Detailed rule documentation |
| `nginx-lint config init` | Generate default config |
| `nginx-lint config schema` | Output JSON Schema for config file |
| `nginx-lint config schema --format markdown` | Configuration reference in Markdown |
