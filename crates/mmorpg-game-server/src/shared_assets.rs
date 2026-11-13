//! The native host's immutable presentation-art selection, using asset-tooling's existing
//! gltf-resources-v1 bundle format. No rules, physics or private records are transferable.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

pub const CAT: &str = "greyhaven.ambient.cat";
pub const CHUNK_BYTES: usize = 16_384;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetFile {
    pub path: String,
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostManifest {
    pub content_revision: u64,
    /// Existing asset-tooling manifest, unchanged, with its canonical JSON identity.
    pub bundle_manifest: serde_json::Value,
    pub bundle_sha256: String,
    /// The selected entry's closure; lengths for resources come from the bundle.
    pub files: Vec<AssetFile>,
}

impl HostManifest {
    pub fn identity(&self) -> Result<String, String> {
        Ok(digest(
            &serde_json::to_vec(self).map_err(|e| e.to_string())?,
        ))
    }
    pub fn validate(&self, revision: u64) -> Result<(), String> {
        if self.content_revision != revision {
            return Err("Host art belongs to another world content revision.".into());
        }
        if self.bundle_manifest["schemaVersion"] != 4
            || self.bundle_manifest["profile"] != "gltf-resources-v1"
        {
            return Err("Host art bundle is incompatible with this client.".into());
        }
        if digest(&serde_json::to_vec(&self.bundle_manifest).map_err(|e| e.to_string())?)
            != self.bundle_sha256
        {
            return Err("Host art manifest identity differs from its contents.".into());
        }
        let entry = cat_entry(&self.bundle_manifest)?;
        let path = entry["path"].as_str().ok_or("Missing cat glTF path")?;
        let hash = entry["sha256"].as_str().ok_or("Missing cat identity")?;
        let resources = entry["resources"]
            .as_array()
            .ok_or("Missing cat resource closure")?;
        if self.files.len() != resources.len() + 1 || self.files.len() > 16 {
            return Err("Invalid cat resource closure".into());
        }
        for file in &self.files {
            if file.byte_length == 0
                || file.byte_length > MAX_FILE_BYTES
                || !valid_hash(&file.sha256)
                || !safe_path(&file.path)
            {
                return Err("Invalid immutable asset reference".into());
            }
        }
        if self.files[0].path != path || self.files[0].sha256 != hash {
            return Err("Cat entry differs from its source bundle".into());
        }
        for (file, resource) in self.files[1..].iter().zip(resources) {
            if resource["path"].as_str() != Some(&file.path)
                || resource["sha256"].as_str() != Some(&file.sha256)
                || resource["byteLength"].as_u64() != Some(file.byte_length)
            {
                return Err("Resource differs from its source bundle".into());
            }
        }
        Ok(())
    }
}
fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
fn cat_entry(bundle: &serde_json::Value) -> Result<&serde_json::Value, String> {
    bundle["assets"]
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["key"] == CAT && entry["variant"] == "default")
        })
        .ok_or_else(|| "The host bundle does not contain the real Greyhaven cat.".into())
}

#[derive(Clone)]
pub struct Catalogue {
    pub manifest: HostManifest,
    files: BTreeMap<String, Arc<[u8]>>,
}
impl Catalogue {
    pub fn open(directory: &Path, revision: u64) -> Result<Self, String> {
        let bytes = fs::read(directory.join("manifest.json"))
            .map_err(|e| format!("Host art manifest unavailable: {e}"))?;
        if bytes.len() > CHUNK_BYTES / 2 {
            return Err("Host art manifest exceeds the bounded content response".into());
        }
        let bundle: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let entry = cat_entry(&bundle)?;
        let path = entry["path"].as_str().ok_or("Missing cat path")?.to_owned();
        if !safe_path(&path) {
            return Err("Invalid cat path".into());
        }
        let mut declared = vec![AssetFile {
            path: path.clone(),
            sha256: entry["sha256"].as_str().ok_or("Missing cat hash")?.into(),
            byte_length: fs::metadata(directory.join(&path))
                .map_err(|e| e.to_string())?
                .len(),
        }];
        for resource in entry["resources"]
            .as_array()
            .ok_or("Missing cat resources")?
        {
            declared.push(AssetFile {
                path: resource["path"]
                    .as_str()
                    .ok_or("Missing resource path")?
                    .into(),
                sha256: resource["sha256"]
                    .as_str()
                    .ok_or("Missing resource hash")?
                    .into(),
                byte_length: resource["byteLength"]
                    .as_u64()
                    .ok_or("Missing resource length")?,
            });
        }
        let manifest = HostManifest {
            content_revision: revision,
            bundle_manifest: bundle.clone(),
            bundle_sha256: digest(&serde_json::to_vec(&bundle).map_err(|e| e.to_string())?),
            files: declared,
        };
        manifest.validate(revision)?;
        let mut files = BTreeMap::new();
        for file in &manifest.files {
            let bytes = fs::read(directory.join(&file.path)).map_err(|e| e.to_string())?;
            verify(file, &bytes)?;
            files.insert(file.sha256.clone(), Arc::from(bytes));
        }
        Ok(Self { manifest, files })
    }
    /// Opaque game-owned content requests: 0 manifest, 1 hash (64 ASCII) + offset u64 BE.
    pub fn response(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        match request {
            [0] => {
                let bytes = serde_json::to_vec(&self.manifest).map_err(|e| e.to_string())?;
                if bytes.len() > CHUNK_BYTES {
                    return Err("Host manifest response is too large".into());
                }
                Ok(bytes)
            }
            [1, rest @ ..] if rest.len() == 72 => {
                let hash = std::str::from_utf8(&rest[..64]).map_err(|_| "Invalid asset hash")?;
                let offset =
                    u64::from_be_bytes(rest[64..].try_into().map_err(|_| "Invalid chunk offset")?);
                let bytes = self
                    .files
                    .get(hash)
                    .ok_or("Asset is not authorized by this manifest")?;
                let offset = usize::try_from(offset).map_err(|_| "Invalid chunk offset")?;
                if offset >= bytes.len() {
                    return Err("Chunk offset is outside the asset".into());
                }
                Ok(bytes[offset..bytes.len().min(offset.saturating_add(CHUNK_BYTES))].to_vec())
            }
            _ => Err("Unsupported content request".into()),
        }
    }
}

pub fn chunk_request(file: &AssetFile, offset: usize) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(file.sha256.as_bytes());
    bytes.extend_from_slice(&(offset as u64).to_be_bytes());
    bytes
}
pub fn verify(file: &AssetFile, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 != file.byte_length || digest(bytes) != file.sha256 {
        return Err(format!(
            "World detail failed hash verification: {}. Retry the download; unverified bytes were not used.",
            file.path
        ));
    }
    Ok(())
}
