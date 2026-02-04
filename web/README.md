# nginx-lint Web

This allows you to try nginx-lint in your browser.

## Quick Start

1. Build the WASM package:
   ```bash
   cargo install wasm-pack
   wasm-pack build --target web --no-default-features --features wasm
   ```

2. Start the web server:
   ```bash
   cargo run --features web-server -- web
   # Or with auto-open browser:
   cargo run --features web-server -- web --open
   ```

3. Open http://localhost:8080/ in your browser.

## Alternative: Manual Server

If you prefer to use your own HTTP server:

```bash
# Using Python
python3 -m http.server 8080

# Or using Node.js
npx serve .
```

Then open http://localhost:8080/web/ in your browser.

## Features

- Real-time linting as you type
- Color-coded results (errors, warnings, info)
- Line number references
- All lint rules available in the CLI are also available here

## Notes

- The `include` directive is not functional in the browser version (no filesystem access)
- The `--fix` feature is not available in the browser version
