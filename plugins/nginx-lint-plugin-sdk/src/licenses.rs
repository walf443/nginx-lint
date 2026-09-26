//! Third-party notices: first for the code compiled into the Lua runtime,
//! which travels inside this binary and inside every plugin it builds, then
//! for the Rust crates compiled into this binary alone. The runtime's texts
//! are the verbatim files under `licenses/`; see the README there for where
//! each came from. The crates' are `licenses/THIRD-PARTY-NOTICES`, generated
//! by cargo-about (`make build-licenses` at the repository root).

/// One embedded component: its name, what of it is in the runtime, and its
/// license text.
pub struct Notice {
    pub component: &'static str,
    pub scope: &'static str,
    pub text: &'static str,
}

pub const NOTICES: &[Notice] = &[
    Notice {
        component: "Lua 5.4",
        scope: "the interpreter and its base, coroutine, table, string, math and utf8 libraries, \
                in the runtime and, for checking scripts at build time, in this binary itself",
        text: include_str!("../licenses/lua/COPYRIGHT"),
    },
    Notice {
        component: "wasi-libc",
        scope: "the C library the runtime is linked against, used under its MIT option; \
                its own overview notice follows, then the MIT text",
        text: include_str!("../licenses/wasi-libc/LICENSE"),
    },
    Notice {
        component: "wasi-libc (MIT License)",
        scope: "wasi-libc's own code, including libsetjmp",
        text: include_str!("../licenses/wasi-libc/LICENSE-MIT"),
    },
    Notice {
        component: "musl",
        scope: "the libc implementation wasi-libc is built from",
        text: include_str!("../licenses/wasi-libc/musl-COPYRIGHT"),
    },
    Notice {
        component: "cloudlibc",
        scope: "the WASI system-call layer of wasi-libc",
        text: include_str!("../licenses/wasi-libc/cloudlibc-LICENSE"),
    },
    Notice {
        component: "LLVM compiler-rt",
        scope: "the compiler builtins (libclang_rt.builtins) linked into the runtime",
        text: include_str!("../licenses/llvm/compiler-rt-LICENSE.TXT"),
    },
];

/// The license texts of the Rust crates compiled into this binary, grouped
/// by license. The plugins it builds contain none of them.
pub const CRATE_NOTICES: &str = include_str!("../licenses/THIRD-PARTY-NOTICES");

/// Renders every runtime notice, each under a heading naming the component,
/// followed by the crates' notices.
pub fn render() -> String {
    let mut out = String::from(
        "nginx-lint-plugin-sdk embeds a Lua runtime, compiled to WebAssembly, in this \
         binary and in every plugin it builds. The runtime contains the \
         following third-party code.\n",
    );
    for notice in NOTICES {
        out.push_str("\n\n");
        out.push_str(&"=".repeat(72));
        out.push('\n');
        out.push_str(notice.component);
        out.push('\n');
        out.push_str(notice.scope);
        out.push('\n');
        out.push_str(&"=".repeat(72));
        out.push_str("\n\n");
        out.push_str(notice.text.trim_end());
        out.push('\n');
    }
    out.push_str("\n\n");
    out.push_str(&"#".repeat(72));
    out.push_str(
        "\n\nThe Rust crates below are in the nginx-lint-plugin-sdk binary only; \
         the plugins it builds contain none of them.\n\n",
    );
    out.push_str(CRATE_NOTICES);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_component_is_rendered_with_its_text() {
        let rendered = render();
        for notice in NOTICES {
            assert!(rendered.contains(notice.component));
            assert!(rendered.contains(notice.text.trim_end()));
        }
        assert!(rendered.ends_with(CRATE_NOTICES));
        // The crates' notices name the binary's dependencies
        assert!(
            CRATE_NOTICES
                .split_whitespace()
                .any(|word| word == "wit-component")
        );
    }
}
