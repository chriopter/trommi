//! The desktop's services the app core asks for (core/client/README.md, "The app core"): the files it hands over
//! for drawing (`FileReady`, `FileGone`), a file offered to the person (`SaveFile`), a link opened outside the app
//! (`OpenUrl`), a notification (`Notify`). Each is done as asked; what to show, save or open is the core's word.
//!
//! - Files for drawing lie in `$XDG_RUNTIME_DIR/trommi-linux/files/` (a tmpfs of this user, gone at logout), else
//!   the app's cache folder, each folder mode 0700 and each file 0600, named by the core's file id (escaped). The
//!   window reads them by `file://` address: [`Desktop::files_json`] maps each file id to its address.
//! - A saved file goes into the person's Downloads folder under a free name.
//! - A link is opened with `xdg-open`, and only an `https:`, `http:` or `mailto:` link.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use trommi_client_core::app::Notice;

/// The largest file the person may pick to attach (the core's limit for one file, 64 MiB of plaintext).
pub const ATTACH_MAX: u64 = 64 << 20;

/// What the desktop does for the window, and what the window is told back.
pub struct Desktop {
    files_dir: PathBuf,
    downloads: PathBuf,
    files: BTreeMap<String, PathBuf>,
    /// The files' map for the window (`files_json`), after every change.
    pub on_files: Box<dyn FnMut(String) + Send>,
    /// A file was saved: where.
    pub on_saved: Box<dyn FnMut(PathBuf) + Send>,
    /// A notification to show.
    pub on_notice: Box<dyn FnMut(Notice) + Send>,
    /// A link to open (checked already).
    pub on_open: Box<dyn FnMut(String) + Send>,
}

impl Desktop {
    /// The desktop's own folders and ways: notifications are dropped until `on_notice` is set.
    pub fn new() -> Desktop {
        Desktop::with_dirs(files_dir(), downloads_dir())
    }

    pub fn with_dirs(files_dir: PathBuf, downloads: PathBuf) -> Desktop {
        Desktop {
            files_dir,
            downloads,
            files: BTreeMap::new(),
            on_files: Box::new(|_| {}),
            on_saved: Box::new(|_| {}),
            on_notice: Box::new(|_| {}),
            on_open: Box::new(|url| {
                let _ = std::process::Command::new("xdg-open")
                    .arg(url)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }),
        }
    }

    /// Keeps a file's bytes for drawing.
    pub fn file_ready(&mut self, file_id: &str, bytes: &[u8]) -> std::io::Result<()> {
        private_dir(&self.files_dir)?;
        let path = self.files_dir.join(escape(file_id));
        write_private(&path, bytes)?;
        self.files.insert(file_id.to_string(), path);
        let json = self.files_json();
        (self.on_files)(json);
        Ok(())
    }

    /// Drops a file's bytes.
    pub fn file_gone(&mut self, file_id: &str) {
        if let Some(path) = self.files.remove(file_id) {
            let _ = std::fs::remove_file(path);
            let json = self.files_json();
            (self.on_files)(json);
        }
    }

    /// Puts a file into Downloads under a free name; its path.
    pub fn save_file(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.downloads)?;
        let path = free_path(&self.downloads, &safe_name(name));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
        file.write_all(bytes)?;
        (self.on_saved)(path.clone());
        Ok(path)
    }

    /// Opens a link outside the app: only `https:`, `http:` and `mailto:`. Whether it was handed on.
    pub fn open_url(&mut self, url: &str) -> bool {
        let lower = url.to_ascii_lowercase();
        let fine = ["https://", "http://", "mailto:"].iter().any(|s| lower.starts_with(s)) && !url.chars().any(|c| c.is_control());
        if fine {
            (self.on_open)(url.to_string());
        }
        fine
    }

    pub fn notify(&mut self, notice: Notice) {
        (self.on_notice)(notice);
    }

    /// Each kept file's id and its `file://` address, as JSON.
    pub fn files_json(&self) -> String {
        let map: serde_json::Map<String, serde_json::Value> = self
            .files
            .iter()
            .map(|(id, path)| (id.clone(), serde_json::Value::String(format!("file://{}", path.display()))))
            .collect();
        serde_json::Value::Object(map).to_string()
    }

    /// Drops every kept file (the app ends).
    pub fn clear(&mut self) {
        for (_, path) in std::mem::take(&mut self.files) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Default for Desktop {
    fn default() -> Self {
        Desktop::new()
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Where the plain bytes of files for drawing lie.
pub fn files_dir() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d).join("trommi-linux/files"),
        _ => crate::shell::base_dir().join("files"),
    }
}

/// The person's Downloads folder: `XDG_DOWNLOAD_DIR` (the environment, then `user-dirs.dirs`), else `~/Downloads`.
pub fn downloads_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
    let expand = |v: &str| -> Option<PathBuf> {
        let v = v.trim().trim_matches('"');
        let p = match v.strip_prefix("$HOME") {
            Some(rest) => home.join(rest.trim_start_matches('/')),
            None => PathBuf::from(v),
        };
        (p.is_absolute() && p != home).then_some(p)
    };
    if let Some(p) = std::env::var("XDG_DOWNLOAD_DIR").ok().and_then(|v| expand(&v)) {
        return p;
    }
    let config = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".config"));
    if let Ok(text) = std::fs::read_to_string(config.join("user-dirs.dirs")) {
        if let Some(p) = text.lines().find_map(|l| l.trim().strip_prefix("XDG_DOWNLOAD_DIR=").and_then(expand)) {
            return p;
        }
    }
    home.join("Downloads")
}

/// A picked file, read for `Action::Attach`: its name, media type and bytes. A `file://` address or a path.
pub fn read_picked(url: &str) -> Result<(String, String, Vec<u8>), String> {
    let path = PathBuf::from(percent_decode(url.strip_prefix("file://").unwrap_or(url)));
    let meta = std::fs::metadata(&path).map_err(|e| e.kind().to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > ATTACH_MAX {
        return Err("too large".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.kind().to_string())?;
    let name = safe_name(path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
    let media = media_type(&name).to_string();
    Ok((name, media, bytes))
}

/// The media type a file's name says.
pub fn media_type(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// A file id as a file name: every byte that is not a letter, a digit, `-` or `_` as `%XX`.
fn escape(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    if out.is_empty() {
        out.push_str("%00");
    }
    out.chars().take(200).collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("x"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A name that is safe to put on disk: no folder, no control character, not hidden.
pub fn safe_name(name: &str) -> String {
    let base = Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control() && *c != '/' && *c != '\\').take(96).collect();
    if clean.is_empty() || clean.starts_with('.') {
        "file".into()
    } else {
        clean
    }
}

/// `name` in `dir`, or "name (2).ext" and so on where that is taken.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..1000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("part");
    {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
    }
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_in_their_folder() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name(".hidden"), "file");
        assert_eq!(escape("../a b"), "%2E%2E%2Fa%20b");
        assert_eq!(media_type("Hero.PNG"), "image/png");
        assert_eq!(percent_decode("/tmp/a%20b.png"), "/tmp/a b.png");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn a_saved_file_takes_a_free_name() {
        let dir = std::env::temp_dir().join(format!("trommi-linux-free-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.pdf"), dir.join("a.pdf"));
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.pdf"), dir.join("a (2).pdf"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
