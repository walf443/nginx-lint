# nginx-lint-parser

[![API Docs](https://img.shields.io/badge/docs-GitHub%20Pages-blue)](https://walf443.github.io/nginx-lint/api/nginx_lint_parser/)

nginx configuration file parser for [nginx-lint](https://github.com/walf443/nginx-lint).

## Overview

This crate provides a parser for nginx configuration files that accepts any directive name, allowing extension modules like ngx_headers_more, lua-nginx-module, etc. to be parsed and linted.

## Usage

```rust
use nginx_lint_parser::{parse_string, parse_config};

// Parse from string
let config = parse_string("server { listen 80; }").unwrap();

// Parse from file
let config = parse_config(std::path::Path::new("/etc/nginx/nginx.conf")).unwrap();

// Iterate over all directives recursively
for directive in config.all_directives() {
    println!("{}", directive.name);
}
```

## License

MIT
