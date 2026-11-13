//! Verified native art cache. Only bytes named by the authenticated host manifest become
//! importable. The renderer consumes these bytes; there is no embedded cat fallback.
use mmorpg_game_server::shared_assets::{AssetFile, HostManifest, chunk_request, verify};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

pub struct Cache {
    directory: PathBuf,
}
mod metadata {
    include!(concat!(env!("OUT_DIR"), "/shared_art.rs"));
}
/// Hash-verified closure of the real supported asset, ready for the existing importer.
pub struct VerifiedBundle {
    pub(crate) manifest: HostManifest,
    pub(crate) files: Vec<(String, Vec<u8>)>,
}
impl VerifiedBundle {
    pub fn from_catalogue(
        catalogue: &mmorpg_game_server::shared_assets::Catalogue,
        revision: u64,
    ) -> Result<Self, String> {
        let manifest = catalogue.manifest.clone();
        compatible(&manifest, revision)?;
        let mut files = Vec::new();
        for file in &manifest.files {
            let mut bytes = Vec::new();
            while (bytes.len() as u64) < file.byte_length {
                bytes.extend(catalogue.response(&chunk_request(file, bytes.len()))?);
            }
            verify(file, &bytes)?;
            files.push((file.path.clone(), bytes));
        }
        Ok(Self { manifest, files })
    }
}
fn compatible(manifest: &HostManifest, revision: u64) -> Result<(), String> {
    manifest.validate(revision)?;
    if manifest.bundle_sha256 != metadata::BUNDLE_IDENTITY {
        return Err("This host uses another art release. Install the matching native client; no incompatible detail was imported.".into());
    }
    Ok(())
}
#[derive(Clone, Default, Debug)]
pub struct Receipt {
    pub cache_hits: usize,
    pub files_requested: usize,
    pub bytes_received: usize,
    pub files_verified: usize,
    pub manifest_identity: String,
}

impl Cache {
    pub fn open(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        Ok(Self {
            directory: directory.into(),
        })
    }
    fn load(&self, file: &AssetFile) -> Result<Option<Vec<u8>>, String> {
        let path = self.directory.join(&file.sha256);
        match fs::read(&path) {
            Ok(bytes) if verify(file, &bytes).is_ok() => Ok(Some(bytes)),
            Ok(_) => {
                fs::remove_file(path).map_err(|e| e.to_string())?;
                Ok(None)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn install(&self, file: &AssetFile, bytes: &[u8]) -> Result<(), String> {
        verify(file, bytes)?;
        let temporary =
            tempfile::NamedTempFile::new_in(&self.directory).map_err(|e| e.to_string())?;
        temporary
            .as_file()
            .write_all(bytes)
            .map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        temporary
            .persist(self.directory.join(&file.sha256))
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        File::open(&self.directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn retrieve<F, R>(
        &self,
        manifest: HostManifest,
        revision: u64,
        mut request: F,
    ) -> Result<(VerifiedBundle, Receipt), String>
    where
        F: FnMut(Vec<u8>) -> R,
        R: std::future::Future<Output = Result<Vec<u8>, String>>,
    {
        compatible(&manifest, revision)?;
        let mut receipt = Receipt {
            manifest_identity: manifest.identity()?,
            ..Receipt::default()
        };
        let mut files = Vec::new();
        for file in &manifest.files {
            let bytes = match self.load(file)? {
                Some(bytes) => {
                    receipt.cache_hits += 1;
                    bytes
                }
                None => {
                    receipt.files_requested += 1;
                    let mut bytes = Vec::new();
                    while (bytes.len() as u64) < file.byte_length {
                        let chunk = request(chunk_request(file, bytes.len())).await?;
                        if chunk.is_empty()
                            || chunk.len() as u64 > file.byte_length - bytes.len() as u64
                        {
                            return Err(
                                "Invalid world detail chunk; no bytes were imported.".into()
                            );
                        }
                        receipt.bytes_received += chunk.len();
                        bytes.extend_from_slice(&chunk);
                    }
                    self.install(file, &bytes)?;
                    bytes
                }
            };
            verify(file, &bytes)?;
            receipt.files_verified += 1;
            files.push((file.path.clone(), bytes));
        }
        Ok((VerifiedBundle { manifest, files }, receipt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_game_server::shared_assets::Catalogue;

    #[tokio::test]
    async fn real_cat_cache_verifies_cold_warm_and_corrupt_bytes() {
        let catalogue = Catalogue::open(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../web/public/scenery/greyhaven.ambient-animals"),
            73,
        )
        .expect("installed real cat");
        let directory = tempfile::tempdir().unwrap();
        let cache = Cache::open(directory.path()).unwrap();
        let (_, cold) = cache
            .retrieve(catalogue.manifest.clone(), 73, |request| {
                std::future::ready(catalogue.response(&request))
            })
            .await
            .unwrap();
        assert_eq!(cold.files_requested, 2);
        assert!(cold.bytes_received > 146_384);
        let (_, warm) = cache
            .retrieve(catalogue.manifest.clone(), 73, |_| async {
                panic!("a warm verified cache never requests file bytes")
            })
            .await
            .unwrap();
        assert_eq!((warm.cache_hits, warm.bytes_received), (2, 0));
        let file = &catalogue.manifest.files[1];
        fs::write(directory.path().join(&file.sha256), b"corrupt cache").unwrap();
        let (_, repaired) = cache
            .retrieve(catalogue.manifest.clone(), 73, |request| {
                std::future::ready(catalogue.response(&request))
            })
            .await
            .unwrap();
        assert_eq!((repaired.cache_hits, repaired.files_requested), (1, 1));
        fs::remove_file(directory.path().join(&file.sha256)).unwrap();
        let refused = cache
            .retrieve(catalogue.manifest.clone(), 73, |request| {
                let mut response = catalogue.response(&request).unwrap();
                response[0] ^= 1;
                std::future::ready(Ok(response))
            })
            .await;
        assert!(refused.err().unwrap().contains("hash verification"));
        assert!(!directory.path().join(&file.sha256).exists());
        let mismatch = cache
            .retrieve(catalogue.manifest.clone(), 74, |_| async {
                panic!("incompatible world revisions must fail before any request")
            })
            .await;
        assert!(mismatch.err().unwrap().contains("content revision"));
    }
}
