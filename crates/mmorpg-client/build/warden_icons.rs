//! Fixed native consumer selection from the same owner-approved browser package.
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub const MANIFEST_SHA256: &str =
    "a59d54c4191a07de613769726ad1c41513f5256f0289281fb0b000e7fcdeddc5";
pub fn validate(directory: &Path) -> Result<(), String> {
    let bytes =
        fs::read(directory.join("producer-manifest.json")).map_err(|cause| cause.to_string())?;
    if bytes.len() != 53124 || digest(&bytes) != MANIFEST_SHA256 {
        return Err("Warden icon manifest differs from the owner-approved pin".into());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|cause| cause.to_string())?;
    if manifest["schemaVersion"] != 3 || manifest["profile"] != "approved-png-v1" {
        return Err("Warden icons require the approved-png-v1 producer export".into());
    }
    let assets = manifest["assets"]
        .as_array()
        .ok_or("Warden manifest has no assets")?;
    for ability in 1..=4 {
        for size in ["64", "512"] {
            let key = format!("warden.ability.{ability}");
            let asset = assets
                .iter()
                .find(|entry| entry["key"] == key && entry["variant"] == size)
                .ok_or_else(|| format!("Missing approved Warden image {key}/{size}"))?;
            let approved = asset["approved"]["sha256"]
                .as_str()
                .ok_or("Warden image has no approved hash")?;
            if asset["source"]["sha256"] != approved || asset["source"]["mediaType"] != "image/png"
            {
                return Err(format!("Invalid approved Warden image {key}/{size}"));
            }
            let filename = format!("warden-{ability}-{size}.png");
            let bytes = fs::read(directory.join(&filename))
                .map_err(|cause| format!("{filename}: {cause}"))?;
            if asset["source"]["byteLength"].as_u64() != Some(bytes.len() as u64)
                || digest(&bytes) != approved
            {
                return Err(format!(
                    "Warden image {filename} differs from its approved bytes"
                ));
            }
        }
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
