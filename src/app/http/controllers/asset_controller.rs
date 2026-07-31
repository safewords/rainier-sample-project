//! `AssetController` — the built frontend's files.
//!
//! Rainier is the web server, so `public/build` needs a route: Vite writes
//! content-hashed files there and the `@vite` directive emits URLs under
//! `/build/…`. Costs one route entry and answers 404 until a build exists,
//! which is exactly what an application that never runs Vite wants.

use std::path::{Component, Path, PathBuf};

use rainier_framework::prelude::*;

/// `GET /build/{path*}` — one file from `public/build`.
pub async fn build(request: Req) -> Result<Response> {
    let requested = request
        .route_param("path")
        .ok_or_else(|| Error::not_found("No route matched."))?
        .to_string();

    // Refused, not 404'd differently: a path that escaped the directory is an
    // attempt, and the answer to an attempt is the same as the answer to a
    // typo.
    let Some(path) = resolve_within(Path::new("public/build"), &requested) else {
        tracing::warn!(%requested, "refused an asset path that left public/build");
        return Err(Error::not_found("No route matched."));
    };

    let Ok(bytes) = std::fs::read(&path) else {
        return Err(Error::not_found("No route matched."));
    };

    Ok(Response::ok(bytes)
        .with_header("content-type", content_type(&path))
        // Vite content-hashes every filename, so a given URL's bytes never
        // change and a year of `immutable` is safe. The manifest itself is
        // never served — the resolver reads it from disk.
        .with_header("cache-control", "public, max-age=31536000, immutable"))
}

/// Resolve `requested` under `dir`, or nothing if it escapes.
///
/// By component rather than by canonicalising and comparing prefixes:
/// canonicalisation needs the file to exist, so a traversal at something
/// absent would 404 for the wrong reason — and a string prefix check says
/// `public/build-evil` is inside `public/build`.
fn resolve_within(dir: &Path, requested: &str) -> Option<PathBuf> {
    let mut path = dir.to_path_buf();
    let mut depth = 0usize;

    for component in Path::new(requested).components() {
        match component {
            Component::Normal(part) => {
                path.push(part);
                depth += 1;
            }
            Component::ParentDir => {
                depth = depth.checked_sub(1)?;
                path.pop();
            }
            Component::CurDir => {}
            // A rooted or prefixed component restarts the path somewhere
            // else entirely; nothing legitimate produces one.
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    (depth > 0).then_some(path)
}

/// The content type, by extension — the closed set Vite actually emits.
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or_default() {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "map" | "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversals_are_refused_and_plain_paths_resolve() {
        let dir = Path::new("public/build");

        assert_eq!(
            resolve_within(dir, "assets/app-Bx91.js"),
            Some(dir.join("assets").join("app-Bx91.js")),
        );
        assert_eq!(resolve_within(dir, "../secrets.txt"), None);
        assert_eq!(resolve_within(dir, "a/../../secrets.txt"), None);
        assert_eq!(resolve_within(dir, "/etc/passwd"), None);
        assert_eq!(resolve_within(dir, ""), None, "the directory itself is not a file");
    }

    #[test]
    fn content_types_cover_what_vite_emits() {
        assert_eq!(content_type(Path::new("a/app-x.js")), "text/javascript; charset=utf-8");
        assert_eq!(content_type(Path::new("a/app-x.css")), "text/css; charset=utf-8");
        assert_eq!(content_type(Path::new("a/logo.svg")), "image/svg+xml");
        assert_eq!(content_type(Path::new("a/unknown.bin")), "application/octet-stream");
    }
}
