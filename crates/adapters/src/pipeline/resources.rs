//! Explicit, checksum-pinned native resource acquisition. Never installs a language runtime.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub kind: String,
    pub url: String,
    pub sha256: String,
    pub filename: String,
    #[serde(default)]
    pub archive_path: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub resources: Vec<Resource>,
}
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
fn component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}
pub fn install(rt: &Runtime, catalog: &Catalog, name: &str) -> Result<PathBuf> {
    if catalog.schema != 1 {
        return Err(Error::input("Unsupported resource catalog schema"));
    }
    let resource = catalog
        .resources
        .iter()
        .find(|r| r.name == name && (r.platform == platform() || r.platform == "any"))
        .ok_or_else(|| Error::input("This catalog has no matching resource for this platform"))?;
    if !component(&resource.name)
        || !component(&resource.filename)
        || resource.sha256.len() != 64
        || !resource.sha256.chars().all(|c| c.is_ascii_hexdigit())
        || !["tool", "model", "language"].contains(&resource.kind.as_str())
    {
        return Err(Error::input("Invalid resource catalog entry"));
    }
    config::validate_url(&resource.url)?;
    let url =
        reqwest::Url::parse(&resource.url).map_err(|_| Error::input("Invalid resource URL"))?;
    if url.scheme() != "https"
        && url.host_str() != Some("127.0.0.1")
        && url.host_str() != Some("localhost")
    {
        return Err(Error::input("Resource downloads require HTTPS"));
    }
    let root = config::data_directory().join("resources");
    fs::create_dir_all(&root)?;
    let mut download = tempfile::NamedTempFile::new_in(&root)?;
    let mut response = providers::client(3600)?
        .get(&resource.url)
        .send()
        .map_err(|_| Error::operation("Could not download resource"))?;
    if !response.status().is_success() {
        return Err(Error::operation(format!(
            "Resource download returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        if rt.cancelled() {
            return Err(Error::paused());
        }
        let n = response.read(&mut buffer).map_err(|_| {
            Error::operation(
                "Resource download interrupted; retry to start a fresh verified download",
            )
        })?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > 8 * 1024 * 1024 * 1024 {
            return Err(Error::operation(
                "Resource exceeds the 8 GiB download limit",
            ));
        }
        download.write_all(&buffer[..n])?;
    }
    download.flush()?;
    if file_hash(download.path())? != resource.sha256.to_lowercase() {
        return Err(Error::operation(
            "Resource checksum does not match the pinned catalog. Nothing was installed.",
        ));
    }
    let directory = root.join(match resource.kind.as_str() {
        "tool" => "tools",
        "model" => "models",
        _ => "languages",
    });
    fs::create_dir_all(&directory)?;
    let destination = directory.join(&resource.filename);
    let bytes = if let Some(entry) = &resource.archive_path {
        let mut archive = zip::ZipArchive::new(File::open(download.path())?)
            .map_err(|_| Error::input("Invalid resource ZIP archive"))?;
        let mut file = archive
            .by_name(entry)
            .map_err(|_| Error::input("Resource file is absent from ZIP archive"))?;
        if file.size() > 8 * 1024 * 1024 * 1024
            || file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err(Error::input("Invalid archived resource"));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        bytes
    } else {
        fs::read(download.path())?
    };
    if resource.kind == "language" {
        return Err(Error::input(
            "Language-pack catalogs must be supplied by the compiled Blitzer distribution; arbitrary pack formats are not guessed",
        ));
    }
    let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
    staged.write_all(&bytes)?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    if resource.kind == "tool" {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o755))?;
        }
    }
    staged
        .persist(&destination)
        .map_err(|e| Error::operation(e.error.to_string()))?;
    write_json(
        &directory.join(format!("{}.resource.json", resource.filename)),
        &json!({"resource":resource,"installed_checksum":file_hash(&destination)?}),
    )?;
    Ok(destination)
}
