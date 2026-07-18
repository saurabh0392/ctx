//! Coarse file-role classifier for Read decisions (CTX-45 / ADR 0030).
//!
//! Maps a file path to one of the model's path-role buckets: `src`, `test`, `config`,
//! `generated`, `vendored`, or `docs`. Used only for logging and training; it never changes
//! trim behavior. Unknown or empty paths return `None`, which leaves the role one-hot all-zero.

/// Return the coarse role for a read path, or `None` when the path is empty or unclassifiable.
pub fn path_role_of(path: &str) -> Option<&'static str> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let lower = path.to_lowercase();
    let p = lower.replace('\\', "/");

    if is_vendored(&p) {
        return Some("vendored");
    }
    if is_generated(&p) {
        return Some("generated");
    }
    if is_test(&p) {
        return Some("test");
    }
    if is_docs(&p) {
        return Some("docs");
    }
    if is_config(&p) {
        return Some("config");
    }
    Some("src")
}

fn is_vendored(p: &str) -> bool {
    const DIRS: &[&str] = &[
        "/node_modules/",
        "/vendor/",
        "/.venv/",
        "/venv/",
        "/site-packages/",
        "/__pycache__/",
        "/.git/",
        "/third_party/",
        "/third-party/",
    ];
    DIRS.iter().any(|d| p.contains(d))
}

fn is_generated(p: &str) -> bool {
    const DIRS: &[&str] = &[
        "/target/",
        "/dist/",
        "/build/",
        "/.next/",
        "/out/",
        "/coverage/",
        "/.cargo/registry/",
        "/.nuxt/",
        "/.svelte-kit/",
    ];
    if DIRS.iter().any(|d| p.contains(d)) {
        return true;
    }
    const SUFFIXES: &[&str] = &[".min.js", ".min.css", ".map", ".lock", ".sum"];
    if SUFFIXES.iter().any(|s| p.ends_with(s)) {
        return true;
    }
    matches!(
        file_name(p),
        Some(
            "package-lock.json"
                | "yarn.lock"
                | "pnpm-lock.yaml"
                | "cargo.lock"
                | "go.sum"
                | "poetry.lock"
                | "composer.lock"
                | "gemfile.lock"
        )
    )
}

fn is_test(p: &str) -> bool {
    const DIRS: &[&str] = &[
        "/test/",
        "/tests/",
        "/__tests__/",
        "/spec/",
        "/specs/",
        "/e2e/",
        "/cypress/",
        "/playwright/",
    ];
    if DIRS.iter().any(|d| p.contains(d)) {
        return true;
    }
    let Some(name) = file_name(p) else {
        return false;
    };
    name.ends_with("_test.rs")
        || name.ends_with("_test.py")
        || name.ends_with("_test.go")
        || name.starts_with("test_")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("_spec.rb")
        || name.ends_with("_test.ts")
        || name.ends_with("_test.tsx")
        || name.ends_with("_test.js")
        || name.ends_with("_test.jsx")
}

fn is_docs(p: &str) -> bool {
    const DIRS: &[&str] = &["/docs/", "/doc/", "/documentation/"];
    if DIRS.iter().any(|d| p.contains(d)) {
        return true;
    }
    let Some(name) = file_name(p) else {
        return false;
    };
    name.ends_with(".md")
        || name.ends_with(".mdx")
        || name.ends_with(".rst")
        || name.ends_with(".adoc")
        || matches!(
            name,
            "readme" | "readme.md" | "changelog" | "changelog.md" | "license" | "license.md"
        )
}

fn is_config(p: &str) -> bool {
    const DIRS: &[&str] = &[
        "/config/",
        "/.github/",
        "/.vscode/",
        "/.cursor/",
        "/infra/",
        "/deploy/",
        "/kubernetes/",
        "/k8s/",
    ];
    if DIRS.iter().any(|d| p.contains(d)) {
        return true;
    }
    let Some(name) = file_name(p) else {
        return false;
    };
    matches!(
        name,
        ".env"
            | ".env.local"
            | ".env.example"
            | "package.json"
            | "tsconfig.json"
            | "jsconfig.json"
            | "cargo.toml"
            | "cargo.lock"
            | "pyproject.toml"
            | "setup.py"
            | "setup.cfg"
            | "go.mod"
            | "dockerfile"
            | "docker-compose.yml"
            | "docker-compose.yaml"
            | "makefile"
            | "justfile"
            | "vite.config.ts"
            | "vite.config.js"
            | "next.config.js"
            | "next.config.ts"
            | "tailwind.config.js"
            | "tailwind.config.ts"
            | "eslint.config.js"
            | "eslint.config.mjs"
            | "prettier.config.js"
            | "wrangler.toml"
            | "fly.toml"
            | "netlify.toml"
            | "vercel.json"
    ) || name.ends_with(".toml")
        || name.ends_with(".yaml")
        || name.ends_with(".yml")
        || (name.ends_with(".json") && !name.contains(".min.") && !name.ends_with(".map"))
}

fn file_name(p: &str) -> Option<&str> {
    p.rsplit('/').next().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_vendored_and_generated() {
        assert_eq!(
            path_role_of("/proj/node_modules/react/index.js"),
            Some("vendored")
        );
        assert_eq!(path_role_of("web/.next/server/chunk.js"), Some("generated"));
        assert_eq!(
            path_role_of("/proj/target/debug/build.rs"),
            Some("generated")
        );
    }

    #[test]
    fn classifies_test_and_docs() {
        assert_eq!(path_role_of("src/foo_test.rs"), Some("test"));
        assert_eq!(path_role_of("src/components/App.test.tsx"), Some("test"));
        assert_eq!(path_role_of("docs/adr/0001-read.md"), Some("docs"));
        assert_eq!(path_role_of("README.md"), Some("docs"));
    }

    #[test]
    fn classifies_config_and_src() {
        assert_eq!(path_role_of("Cargo.toml"), Some("config"));
        assert_eq!(path_role_of(".github/workflows/ci.yml"), Some("config"));
        assert_eq!(path_role_of("src/main.rs"), Some("src"));
        assert_eq!(path_role_of("web/components/App.tsx"), Some("src"));
    }

    #[test]
    fn empty_path_is_none() {
        assert_eq!(path_role_of(""), None);
        assert_eq!(path_role_of("   "), None);
    }
}
