//! Benchmark for the directive-inheritance plugin's check() method.
//!
//! Measures performance across different config sizes and nesting depths.
//!
//! Run with:
//!   cargo bench -p directive-inheritance-plugin

use directive_inheritance_plugin::DirectiveInheritancePlugin;
use nginx_lint_plugin::prelude::*;
use std::time::{Duration, Instant};

const ITERATIONS: u32 = 1000;

/// Small config: 1 server, 1 location, a few directives.
const SMALL_CONFIG: &str = r#"
http {
    server {
        listen 80;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#;

/// Medium config: multiple servers, locations, and directive types.
const MEDIUM_CONFIG: &str = r#"
http {
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    add_header X-Frame-Options DENY;
    add_header X-Content-Type-Options nosniff;
    error_page 404 /404.html;
    error_page 500 502 503 504 /50x.html;

    server {
        listen 80;
        server_name example.com;

        proxy_set_header X-Forwarded-Proto $scheme;

        location / {
            proxy_set_header X-Custom "value";
            add_header X-Location "main";
            proxy_pass http://backend;
        }

        location /api {
            proxy_set_header X-API "true";
            fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
            fastcgi_param REQUEST_METHOD $request_method;
            fastcgi_pass unix:/run/php/php-fpm.sock;
        }

        location /static {
            add_header Cache-Control "public, max-age=3600";
            error_page 403 /403.html;
            root /var/www/static;
        }
    }

    server {
        listen 443 ssl;
        server_name secure.example.com;

        proxy_set_header X-Forwarded-Proto https;
        proxy_hide_header X-Powered-By;
        proxy_hide_header Server;

        location / {
            proxy_set_header X-Custom "secure";
            proxy_hide_header X-Debug;
            proxy_pass http://backend;
        }

        location /admin {
            proxy_set_header X-Admin "true";
            add_header X-Admin-Header "yes";
            proxy_pass http://admin-backend;
        }
    }
}
"#;

/// Large config: deeply nested, many servers, many directive types.
const LARGE_CONFIG: &str = r#"
http {
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    add_header X-Frame-Options SAMEORIGIN;
    add_header X-Content-Type-Options nosniff;
    add_header X-XSS-Protection "1; mode=block";
    add_header Strict-Transport-Security "max-age=31536000";
    proxy_hide_header X-Powered-By;
    error_page 404 /404.html;
    error_page 500 502 503 504 /50x.html;

    server {
        listen 80;
        server_name app1.example.com;
        proxy_set_header X-Server app1;
        add_header X-Server-Id app1;

        location / {
            proxy_set_header X-Location root;
            proxy_pass http://app1-backend;
        }
        location /api/v1 {
            proxy_set_header X-API v1;
            add_header X-API-Version v1;
            proxy_pass http://api-v1;

            location /api/v1/admin {
                proxy_set_header X-Admin "true";
                proxy_pass http://admin;
            }
        }
        location /api/v2 {
            proxy_set_header X-API v2;
            add_header X-API-Version v2;
            proxy_pass http://api-v2;
        }
        location ~ \.php$ {
            fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
            fastcgi_param REQUEST_METHOD $request_method;
            fastcgi_param QUERY_STRING $query_string;
            fastcgi_pass unix:/run/php/php-fpm.sock;
        }
        location /static {
            add_header Cache-Control "public, max-age=86400";
            error_page 403 /403.html;
            root /var/www/static;
        }
    }

    server {
        listen 80;
        server_name app2.example.com;
        proxy_set_header X-Server app2;
        add_header X-Server-Id app2;
        proxy_hide_header X-Debug;

        location / {
            proxy_set_header X-Location root;
            proxy_hide_header X-Internal;
            proxy_pass http://app2-backend;
        }
        location /api {
            proxy_set_header X-API "true";
            add_header X-API "true";
            proxy_pass http://app2-api;

            location /api/internal {
                proxy_set_header X-Internal "true";
                proxy_pass http://internal;
            }
        }
        location /uploads {
            add_header Content-Disposition attachment;
            error_page 413 /413.html;
            root /var/www/uploads;
        }
    }

    server {
        listen 443 ssl;
        server_name secure.example.com;
        proxy_set_header X-Server secure;
        add_header X-Server-Id secure;
        proxy_hide_header Server;
        proxy_hide_header X-Powered-By;
        error_page 403 /403.html;

        location / {
            proxy_set_header X-Location root;
            add_header X-Loc root;
            proxy_pass http://secure-backend;
        }
        location /admin {
            proxy_set_header X-Admin "true";
            add_header X-Admin-Panel "true";
            proxy_hide_header X-Debug;
            error_page 401 /401.html;
            proxy_pass http://admin;

            if ($request_method = POST) {
                proxy_set_header X-Method POST;
            }
        }
        location /api {
            proxy_set_header X-API "true";
            proxy_pass http://secure-api;
        }
        location ~ \.php$ {
            fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
            fastcgi_param REQUEST_METHOD $request_method;
            fastcgi_param QUERY_STRING $query_string;
            fastcgi_param CONTENT_TYPE $content_type;
            fastcgi_pass unix:/run/php/php-fpm.sock;
        }
    }

    server {
        listen 80;
        server_name legacy.example.com;
        proxy_set_header Host $host;
        add_header X-Legacy "true";

        location / {
            proxy_set_header X-Custom legacy;
            add_header X-Backend legacy;
            proxy_pass http://legacy-backend;
        }
        location /old-api {
            proxy_set_header X-Old "true";
            proxy_pass http://old-api;
        }
    }
}
"#;

fn bench_config(name: &str, config_str: &str) {
    let config =
        nginx_lint_plugin::parse_string(config_str).unwrap_or_else(|e| panic!("{name}: {e}"));
    let plugin = DirectiveInheritancePlugin;

    // Warm up
    let _ = plugin.check(&config, "bench.conf");

    // Measure
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = plugin.check(&config, "bench.conf");
    }
    let total = start.elapsed();
    let per_iter = total / ITERATIONS;

    let errors = plugin.check(&config, "bench.conf");

    println!("  {name:<12} {total:>10.3?} total, {per_iter:>10.3?}/iter  ({} errors)", errors.len());
}

fn bench_scaling() {
    // Generate configs with increasing numbers of servers to measure scaling
    let mut results: Vec<(usize, Duration)> = Vec::new();

    for num_servers in [1, 2, 4, 8, 16] {
        let config_str = generate_config(num_servers);
        let config = nginx_lint_plugin::parse_string(&config_str).unwrap();
        let plugin = DirectiveInheritancePlugin;

        // Warm up
        let _ = plugin.check(&config, "bench.conf");

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = plugin.check(&config, "bench.conf");
        }
        let total = start.elapsed();
        let per_iter = total / ITERATIONS;

        results.push((num_servers, per_iter));
        println!("  {num_servers:>2} servers:  {per_iter:>10.3?}/iter");
    }

    // Show scaling factor
    if results.len() >= 2 {
        let base = results[0].1.as_nanos() as f64;
        println!();
        println!("  Scaling (relative to 1 server):");
        for (servers, dur) in &results {
            let factor = dur.as_nanos() as f64 / base;
            println!("    {servers:>2} servers: {factor:.2}x");
        }
    }
}

fn generate_config(num_servers: usize) -> String {
    let mut config = String::from(
        "http {\n\
         \x20   proxy_set_header Host $host;\n\
         \x20   proxy_set_header X-Real-IP $remote_addr;\n\
         \x20   add_header X-Frame-Options DENY;\n\
         \x20   error_page 404 /404.html;\n\
         \x20   error_page 500 502 503 504 /50x.html;\n\n",
    );

    for i in 0..num_servers {
        config.push_str(&format!(
            "\x20   server {{\n\
             \x20       listen 80;\n\
             \x20       server_name app{i}.example.com;\n\
             \x20       proxy_set_header X-Server app{i};\n\
             \x20       add_header X-Server-Id app{i};\n\n\
             \x20       location / {{\n\
             \x20           proxy_set_header X-Location root;\n\
             \x20           add_header X-Loc root;\n\
             \x20           proxy_pass http://backend-{i};\n\
             \x20       }}\n\
             \x20       location /api {{\n\
             \x20           proxy_set_header X-API \"true\";\n\
             \x20           add_header X-API-Version v1;\n\
             \x20           error_page 403 /403.html;\n\
             \x20           proxy_pass http://api-{i};\n\n\
             \x20           location /api/admin {{\n\
             \x20               proxy_set_header X-Admin \"true\";\n\
             \x20               proxy_pass http://admin-{i};\n\
             \x20           }}\n\
             \x20       }}\n\
             \x20   }}\n\n"
        ));
    }

    config.push_str("}\n");
    config
}

fn main() {
    println!("=== directive-inheritance plugin benchmark ===");
    println!("Iterations: {ITERATIONS}");
    println!();

    println!("--- Config size ---");
    bench_config("small", SMALL_CONFIG);
    bench_config("medium", MEDIUM_CONFIG);
    bench_config("large", LARGE_CONFIG);
    println!();

    println!("--- Scaling (servers) ---");
    bench_scaling();
}
