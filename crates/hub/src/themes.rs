use crate::runtime::Shared;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

const LIMIT: u64 = 64 * 1024 * 1024;
pub fn safe_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.to_string_lossy().contains(['\\', ':'])
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}
fn localized(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("zh-CN")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| value.get("en").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}
pub fn inspect(dir: &Path) -> Result<Value> {
    for (file, profile) in [
        ("monitor-theme.json", "native"),
        ("komari-theme.json", "komari"),
        ("theme.json", "monitor"),
    ] {
        let path = dir.join(file);
        if !path.is_file() {
            continue;
        }
        let manifest: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let short = manifest["short"].as_str().context("主题缺少 short")?;
        ensure!(
            !short.is_empty()
                && short.len() <= 64
                && short
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "主题标识无效"
        );
        ensure!(!localized(&manifest["name"]).is_empty(), "主题缺少名称");
        ensure!(
            dir.join("dist/index.html").is_file(),
            "主题包需要已构建的 dist/index.html"
        );
        return Ok(
            json!({"id":short,"name":localized(&manifest["name"]),"description":localized(&manifest["description"]),"version":manifest["version"],"author":localized(&manifest["author"]),"profile":profile,"manifest":manifest}),
        );
    }
    anyhow::bail!("未找到主题清单文件")
}
pub fn unpack(bytes: &[u8], destination: &Path) -> Result<()> {
    ensure!(bytes.len() <= 32 * 1024 * 1024, "主题包不能超过 32 MiB");
    std::fs::create_dir_all(destination)?;
    let mut total = 0u64;
    let mut files = 0usize;
    if bytes.starts_with(b"PK") {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let path = entry.enclosed_name().context("ZIP 包含非法路径")?;
            ensure!(safe_path(&path), "ZIP 包含非法路径");
            ensure!(
                entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                "主题包不接受链接"
            );
            files += 1;
            ensure!(
                files <= 2000 && entry.size() <= 8 * 1024 * 1024,
                "主题文件过多或过大"
            );
            let out = destination.join(path);
            if entry.is_dir() {
                std::fs::create_dir_all(out)?;
            } else {
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut data = vec![];
                entry
                    .by_ref()
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_end(&mut data)?;
                total += data.len() as u64;
                ensure!(
                    total <= LIMIT && data.len() <= 8 * 1024 * 1024,
                    "主题解压超过限制"
                );
                std::fs::write(out, data)?;
            }
        }
    } else {
        let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            ensure!(safe_path(&path), "tar 包含非法路径");
            let kind = entry.header().entry_type();
            ensure!(
                kind.is_file() || kind.is_dir(),
                "主题包不接受链接或特殊文件"
            );
            files += 1;
            ensure!(
                files <= 2000 && entry.size() <= 8 * 1024 * 1024,
                "主题文件过多或过大"
            );
            let out = destination.join(path);
            if kind.is_dir() {
                std::fs::create_dir_all(out)?;
            } else {
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut data = vec![];
                entry
                    .by_ref()
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_end(&mut data)?;
                total += data.len() as u64;
                ensure!(
                    total <= LIMIT && data.len() <= 8 * 1024 * 1024,
                    "主题解压超过限制"
                );
                std::fs::write(out, data)?;
            }
        }
        // Consume the decoder through EOF so a truncated gzip trailer is rejected.
        let mut decoder = archive.into_inner();
        let mut tail = Vec::new();
        decoder
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut tail)?;
        ensure!(tail.len() <= 1024 * 1024, "归档尾部超过限制");
    }
    Ok(())
}
pub fn import(bytes: Vec<u8>, themes_dir: PathBuf) -> Result<Value> {
    std::fs::create_dir_all(&themes_dir)?;
    let staging = themes_dir.join(format!(".upload-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        unpack(&bytes, &staging)?;
        let root = if inspect(&staging).is_ok() {
            staging.clone()
        } else {
            let candidates = std::fs::read_dir(&staging)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir() && inspect(&e.path()).is_ok())
                .map(|e| e.path())
                .collect::<Vec<_>>();
            ensure!(candidates.len() == 1, "归档中需要恰好一个主题目录");
            candidates[0].clone()
        };
        let mut info = inspect(&root)?;
        let short = info["id"].as_str().unwrap();
        let id = if short == "default" {
            format!("{}-default", info["profile"].as_str().unwrap())
        } else {
            short.to_owned()
        };
        ensure!(id != "native", "该主题标识为系统保留");
        ensure!(
            !themes_dir.join(&id).exists(),
            "该主题已安装，请先删除旧版本"
        );
        std::fs::rename(&root, themes_dir.join(&id))?;
        info["id"] = json!(id);
        Ok(info)
    })();
    let _ = std::fs::remove_dir_all(staging);
    result
}
pub async fn list(app: &Shared) -> Result<Vec<Value>> {
    let mut list = if let Some(root) = builtin_root(app, "default") {
        let mut info = inspect(&root)?;
        info["id"] = json!("default");
        info["name"] = json!("默认主题");
        info["builtin"] = json!(true);
        vec![info]
    } else {
        vec![
            json!({"id":"default","name":"默认主题","description":"","profile":"native","version":env!("CARGO_PKG_VERSION"),"builtin":true}),
        ]
    };
    let builtin = builtin_root(app, "default").and_then(|p| p.canonicalize().ok());
    if let Ok(entries) = std::fs::read_dir(&app.themes_dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') || !entry.path().is_dir() {
                continue;
            }
            if builtin
                .as_ref()
                .is_some_and(|root| entry.path().canonicalize().is_ok_and(|p| p == *root))
            {
                continue;
            }
            if let Ok(mut info) = inspect(&entry.path()) {
                info["id"] = json!(entry.file_name().to_string_lossy());
                list.push(info);
            }
        }
    }
    Ok(list)
}
pub async fn current(app: &Shared) -> Option<Value> {
    let id = app.settings.read().await.theme.clone();
    if id == "native" || (id == "default" && builtin_root(app, &id).is_none()) {
        return None;
    }
    inspect(&root(app, &id)).ok()
}
pub fn builtin_root(app: &Shared, id: &str) -> Option<PathBuf> {
    if id != "default" {
        return None;
    }
    let path = app.frontend_dir.parent()?.join("themes/emerald");
    path.join("dist/index.html").is_file().then_some(path)
}
pub fn root(app: &Shared, id: &str) -> PathBuf {
    builtin_root(app, id).unwrap_or_else(|| app.themes_dir.join(id))
}
pub async fn profile(app: &Shared) -> String {
    current(app)
        .await
        .and_then(|v| v["profile"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "native".into())
}
pub async fn config(app: &Shared, id: &str) -> Result<Value> {
    let stored = app
        .db
        .get(&format!("theme:{id}"))
        .await?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_else(|| json!({}));
    if builtin_root(app, id).is_some() {
        let mut defaults = json!({"visitorInfoCardEnabled":false,"alertEnabled":false});
        if let Some(values) = stored.as_object() {
            defaults.as_object_mut().unwrap().extend(values.clone());
        }
        Ok(defaults)
    } else {
        Ok(stored)
    }
}
pub fn resolve_asset(root: &Path, path: &str) -> Option<PathBuf> {
    let path = Path::new(path.trim_start_matches('/'));
    if !safe_path(path) {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let file = root.join(path).canonicalize().ok()?;
    if file.starts_with(&root) && file.is_file() {
        Some(file)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_paths_cannot_escape_the_theme_root() {
        for path in ["../secret", "/etc/passwd", "C:/windows", "foo\\..\\bar"] {
            assert!(!safe_path(Path::new(path)));
        }
        assert!(safe_path(Path::new("dist/assets/app.js")));
    }
    #[test]
    fn symlinks_are_rejected_before_extraction() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        archive
            .append_link(&mut header, "dist/out", "/etc/passwd")
            .unwrap();
        let data = archive.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &data).unwrap();
        assert!(unpack(&encoder.finish().unwrap(), dir.path()).is_err());
    }
}
