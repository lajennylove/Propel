//! Resolves `asset()` and `@vite()` to URLs, and serves the built files of a
//! memory source.
//!
//! An app has one default source: its own `public` directory. A package adds
//! a namespace, as it adds views, and refers to its files as
//! `ignite::resources/css/app.css`. A namespace's source is a directory, or
//! files compiled into the binary, so a release build can serve them with no
//! files on disk.
//!
//! For a directory source, the conventions are Laravel's Vite plugin:
//!
//! - **Development.** While `vite` runs, the plugin writes its URL to `hot` in
//!   the directory. Every asset then points at the dev server, and `@vite()`
//!   adds the HMR client. Each source has its own `hot` file.
//! - **Production.** After `vite build`, `build/manifest.json` maps each entry
//!   to its hashed file. Images and fonts are copied to `build/` under their
//!   source path, so they resolve by that path.
//!
//! A memory source has the same manifest and files, but no dev server.
//!
//! Each source serves its built files under a URL prefix, `/build` by default.
//! The hot file wins when it exists. Directory sources are read again on every
//! render, so a rebuild or a new dev server takes effect on the next request.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

/// Where one set of built assets comes from.
#[derive(Debug, Clone)]
pub struct AssetSource {
    kind: Kind,
    prefix: String,
}

#[derive(Debug, Clone)]
enum Kind {
    /// A `public` directory, with `build/` and, in development, `hot`.
    Directory(PathBuf),
    /// The manifest and the built files, compiled into the binary.
    Memory {
        manifest: Option<Arc<Value>>,
        files: &'static [(&'static str, &'static [u8])],
    },
}

impl AssetSource {
    /// A `public` directory, which holds `hot` and `build/`.
    pub fn directory(public: impl Into<PathBuf>) -> Self {
        Self {
            kind: Kind::Directory(public.into()),
            prefix: "/build".into(),
        }
    }

    /// Built files compiled into the binary. `manifest` is the contents of
    /// `build/manifest.json`, and `files` are the built files by their path
    /// under `build/`, such as `assets/app-1a2b.js` or `resources/images/logo.svg`.
    /// A manifest that is not valid JSON counts as no manifest.
    pub fn memory(manifest: &str, files: &'static [(&'static str, &'static [u8])]) -> Self {
        Self {
            kind: Kind::Memory {
                manifest: serde_json::from_str(manifest).ok().map(Arc::new),
                files,
            },
            prefix: "/build".into(),
        }
    }

    /// The URL prefix the built files are served under. The default is `/build`.
    /// A trailing slash is removed, so `/admin/build/` and `/admin/build` are the same.
    pub fn url_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into().trim_end_matches('/').to_owned();
        self
    }

    /// The dev server URL, when this directory has a `hot` file.
    fn dev_server(&self) -> Option<String> {
        match &self.kind {
            Kind::Directory(dir) => {
                let url = std::fs::read_to_string(dir.join("hot")).ok()?;
                Some(url.trim().trim_end_matches('/').to_owned())
            }
            Kind::Memory { .. } => None,
        }
    }

    fn manifest(&self) -> Option<Value> {
        match &self.kind {
            Kind::Directory(dir) => {
                let source = std::fs::read_to_string(dir.join("build/manifest.json")).ok()?;
                serde_json::from_str(&source).ok()
            }
            Kind::Memory { manifest, .. } => manifest.as_deref().cloned(),
        }
    }

    /// Whether the built file `rel` exists in this source.
    fn has_file(&self, rel: &str) -> bool {
        match &self.kind {
            Kind::Directory(dir) => dir.join("build").join(rel).is_file(),
            Kind::Memory { files, .. } => files.iter().any(|(path, _)| *path == rel),
        }
    }

    /// The bytes of a built file, for a memory source. A directory source
    /// returns `None`: the app serves its files from disk.
    fn bytes(&self, rel: &str) -> Option<&'static [u8]> {
        match &self.kind {
            Kind::Memory { files, .. } => files
                .iter()
                .find(|(path, _)| *path == rel)
                .map(|(_, bytes)| *bytes),
            Kind::Directory(_) => None,
        }
    }

    fn url(&self, rel: &str) -> Option<String> {
        if !is_relative(rel) {
            return None;
        }

        if let Some(server) = self.dev_server() {
            return Some(format!("{server}/{rel}"));
        }

        let manifest = self.manifest();
        let file = manifest
            .as_ref()
            .and_then(|manifest| manifest.get(rel))
            .and_then(|entry| entry.get("file"))
            .and_then(Value::as_str);
        if let Some(file) = file {
            return Some(format!("{}/{file}", self.prefix));
        }

        // Images and fonts are copied as they are, so they keep their path.
        self.has_file(rel).then(|| format!("{}/{rel}", self.prefix))
    }
}

/// The assets of one app: its own build, and one source per namespace.
///
/// Build one per process and share it. `url`, `tags` and `file` take a name
/// with an optional namespace: `resources/images/logo.svg` is in the app's own
/// build, and `ignite::resources/images/logo.svg` is in the namespace `ignite`.
#[derive(Debug, Clone, Default)]
pub struct Assets {
    default: Option<AssetSource>,
    namespaces: BTreeMap<String, AssetSource>,
}

impl Assets {
    /// The app's own build, in `public`. This is the default source.
    pub fn new(public: impl Into<PathBuf>) -> Self {
        Self {
            default: Some(AssetSource::directory(public)),
            namespaces: BTreeMap::new(),
        }
    }

    /// No default source. Use this when every asset comes from a namespace, so
    /// the app needs no `public` directory.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Adds a namespace. Names are as for views: letters, digits, `_` and `-`.
    pub fn namespace(mut self, name: impl Into<String>, source: AssetSource) -> Self {
        self.namespaces.insert(name.into(), source);
        self
    }

    /// The source a name resolves to, and the name within it.
    fn resolve<'a>(&'a self, name: &'a str) -> Option<(&'a AssetSource, &'a str)> {
        match name.split_once("::") {
            Some((namespace, rel)) => self.namespaces.get(namespace).map(|s| (s, rel)),
            None => self.default.as_ref().map(|source| (source, name)),
        }
    }

    /// The URL of one file, such as `resources/images/logo.svg` or
    /// `ignite::resources/images/logo.svg`. `None` when the file is neither in
    /// the build nor served by the dev server, or its namespace is unknown.
    pub fn url(&self, name: &str) -> Option<String> {
        let (source, rel) = self.resolve(name)?;
        source.url(rel)
    }

    /// The tags that load each entry, for `@vite()`. Each entry resolves through
    /// its own namespace. `None` when an entry is missing from its build.
    pub fn tags(&self, entries: &[String]) -> Option<String> {
        let mut html = String::new();
        let mut hot_servers: Vec<String> = Vec::new();

        for entry in entries {
            let (source, rel) = self.resolve(entry)?;
            if !is_relative(rel) {
                return None;
            }

            if let Some(server) = source.dev_server() {
                if !hot_servers.contains(&server) {
                    html.push_str(&format!(
                        "<script type=\"module\" src=\"{server}/@vite/client\"></script>\n"
                    ));
                    hot_servers.push(server.clone());
                }
                html.push_str(&tag(&format!("{server}/{rel}"), rel));
                continue;
            }

            let manifest = source.manifest()?;
            let info = manifest.get(rel)?;

            // CSS that the entry imports is loaded with it.
            for css in info
                .get("css")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let css = css.as_str()?;
                html.push_str(&tag(&format!("{}/{css}", source.prefix), "style.css"));
            }

            let file = info.get("file")?.as_str()?;
            html.push_str(&tag(&format!("{}/{file}", source.prefix), file));
        }

        Some(html)
    }

    /// The bytes of a built file in a memory source, for the app to serve at
    /// the source's URL prefix. A directory source has no bytes here: the app
    /// serves those from disk.
    pub fn file(&self, name: &str) -> Option<&'static [u8]> {
        let (source, rel) = self.resolve(name)?;
        if !is_relative(rel) {
            return None;
        }
        source.bytes(rel)
    }
}

/// Whether `path` stays inside its source: no leading `/`, and no `..` segment.
fn is_relative(path: &str) -> bool {
    !path.starts_with('/') && !path.split('/').any(|segment| segment == "..")
}

/// A `<link>` for CSS and a module `<script>` for anything else.
fn tag(url: &str, kind: &str) -> String {
    let mut escaped = String::new();
    crate::escape_into(url, &mut escaped);

    if kind.ends_with(".css") {
        format!("<link rel=\"stylesheet\" href=\"{escaped}\">\n")
    } else {
        format!("<script type=\"module\" src=\"{escaped}\"></script>\n")
    }
}
