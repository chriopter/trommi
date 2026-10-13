//! Files (spec 11): a picture or file a card or a Chat line names is fetched from the hub, decrypted by the core and
//! kept where only this user can read it while the session lasts; a file the person adds is encrypted by the core,
//! uploaded, and named in the body by its reference (iOS `RoomActions.fetchAttachment`, `uploadAttachment`).
//!
//! The plain bytes go to `$XDG_RUNTIME_DIR/trommi-linux/files/` (a tmpfs of this user, mode 0700, gone at logout),
//! or the app's cache folder where there is none, named by the file's id and hash: the window shows them from there.

use super::{fault, Engine};
use crate::store;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use trommi_connector::error::{Fault, Result};
use trommi_connector::hub::b64;
use trommi_core::ids::base64url_decode;
use trommi_core_ffi::{FileDecryptor, FileEncryptor, FileRef};

/// The largest file read or written: the core's limit (64 MiB of plaintext), stored with its overhead.
const MAX_STORED: usize = 66 << 20;

/// Where the plain bytes of files shown lately lie.
pub fn plain_dir() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d).join("trommi-linux/files"),
        _ => store::base_dir().join("cache/files"),
    }
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    trommi_connector::util::unhex(s)
}

/// A file name that is safe to put on disk: the reference's name without a folder, or none.
fn safe_name(name: &str) -> String {
    let base = Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control() && *c != '/' && *c != '\\').take(96).collect();
    if clean.is_empty() || clean.starts_with('.') { "file".into() } else { clean }
}

/// The reference's three fields as the core takes them: the id (hex in the model), the key and the hash (base64url).
fn file_ref(r: &Value) -> Result<FileRef> {
    let bad = || Fault::new("bad-argument", "not an attachment reference");
    let id = r["attachment_id"].as_str().and_then(unhex).ok_or_else(bad)?;
    let key = r["file_key"].as_str().and_then(|k| base64url_decode(k).ok()).ok_or_else(bad)?;
    let sha = r["sha256"].as_str().and_then(|k| base64url_decode(k).ok()).ok_or_else(bad)?;
    Ok(FileRef { file_id: id, file_key: key, sha256: sha })
}

impl Engine {
    /// The plain file an attachment reference names, on disk: fetched and checked once, then from there.
    pub async fn fetch_file(&self, r: &Value) -> Result<PathBuf> {
        let file = file_ref(r)?;
        let id = r["attachment_id"].as_str().unwrap_or("");
        let sha = r["sha256"].as_str().unwrap_or("");
        let dir = plain_dir().join(format!("{id}-{}", &sha.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect::<String>()));
        let path = dir.join(safe_name(r["file_name"].as_str().unwrap_or("")));
        if path.is_file() {
            return Ok(path);
        }
        let stored = self.hub.get_file(&b64(&file.file_id), MAX_STORED).await?;
        // (the core checks the stored file against the reference's hash and opens it chunk by chunk)
        let plain = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let d = FileDecryptor::new(file).map_err(fault)?;
            let mut out = d.update(stored).map_err(fault)?;
            out.extend(d.finish().map_err(fault)?);
            Ok(out)
        })
        .await
        .map_err(|_| Fault::new("internal", "the decryption stopped"))??;
        store::private_dir(plain_dir().parent().unwrap_or(&plain_dir())).map_err(Fault::from)?;
        store::private_dir(&plain_dir()).map_err(Fault::from)?;
        store::private_dir(&dir).map_err(Fault::from)?;
        store::write_atomic(&path, &plain).map_err(Fault::from)?;
        Ok(path)
    }

    /// Fetches a file and puts a copy into the person's Downloads folder (a free name there); the copy's path.
    pub async fn save_file(&self, r: &Value) -> Result<PathBuf> {
        let plain = self.fetch_file(r).await?;
        let dir = downloads_dir();
        std::fs::create_dir_all(&dir).map_err(Fault::from)?;
        let name = safe_name(r["file_name"].as_str().unwrap_or(""));
        let to = free_path(&dir, &name);
        std::fs::copy(&plain, &to).map_err(Fault::from)?;
        Ok(to)
    }

    /// Encrypts and uploads a file of this machine; its attachment reference for a body (the model's names).
    pub async fn upload_file(&self, path: &Path) -> Result<Value> {
        let meta = std::fs::metadata(path).map_err(Fault::from)?;
        if meta.len() > (64 << 20) {
            return Err(Fault::new("too-large", "a file may be 64 MB at most"));
        }
        let plain = std::fs::read(path).map_err(Fault::from)?;
        let size = plain.len();
        let (file_id, stored, end) = tokio::task::spawn_blocking(move || -> Result<_> {
            let e = FileEncryptor::new().map_err(fault)?;
            let id = e.file_id().map_err(fault)?;
            let mut stored = Vec::with_capacity(plain.len() + plain.len() / 64 + 64);
            for chunk in plain.chunks(1 << 16) {
                stored.extend(e.update(chunk.to_vec()).map_err(fault)?);
            }
            let end = e.finish().map_err(fault)?;
            stored.extend(&end.stored);
            Ok((id, stored, end))
        })
        .await
        .map_err(|_| Fault::new("internal", "the encryption stopped"))??;
        self.hub.put_file(&b64(&file_id), stored).await?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        Ok(json!({
            "attachment_id": trommi_connector::util::hex(&file_id),
            "file_key": b64(&end.file.file_key),
            "sha256": b64(&end.file.sha256),
            "total_size": size,
            "file_name": name,
            "media_type": media_type(&name),
        }))
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
    (2..1000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// The media type a file's name says (the web takes the browser's; a few the views know by name).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_in_their_folder() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name(".hidden"), "file");
        assert_eq!(safe_name(""), "file");
        assert_eq!(safe_name("hero split.png"), "hero split.png");
        assert_eq!(media_type("Hero.PNG"), "image/png");
    }

    #[test]
    fn a_saved_file_takes_a_free_name() {
        let dir = std::env::temp_dir().join(format!("trommi-linux-free-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.pdf"), dir.join("a.pdf"));
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.pdf"), dir.join("a (2).pdf"));
        std::fs::write(dir.join("notes"), b"x").unwrap();
        assert_eq!(free_path(&dir, "notes"), dir.join("notes (2)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_comes_back_as_it_went() {
        let plain: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let e = FileEncryptor::new().unwrap();
        let mut stored = e.update(plain.clone()).unwrap();
        let end = e.finish().unwrap();
        stored.extend(&end.stored);
        let r = json!({
            "attachment_id": trommi_connector::util::hex(&end.file.file_id),
            "file_key": b64(&end.file.file_key),
            "sha256": b64(&end.file.sha256),
        });
        let d = FileDecryptor::new(file_ref(&r).unwrap()).unwrap();
        let mut out = d.update(stored).unwrap();
        out.extend(d.finish().unwrap());
        assert_eq!(out, plain);
    }
}
