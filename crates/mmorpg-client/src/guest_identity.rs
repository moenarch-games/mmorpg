//! OS-private guest signing identities. Host entitlements remain world-owned; the key
//! proves this local roster slot without sending any private key or bearer credential.
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};

pub fn proof(
    directory: &Path,
    local_id: &str,
    lobby: &str,
    participant: &str,
    matched: &str,
) -> Result<Vec<u8>, String> {
    mmorpg_core::CharacterId::new(local_id.to_owned()).map_err(|e| e.to_string())?;
    let directory = directory.join("guest-identities");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join("identities.lock"))
        .map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let path = directory.join(format!("{local_id}.pk8"));
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .map_err(|_| "Guest key generation failed")?;
            let temporary = path.with_extension("tmp");
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
            file.write_all(key.as_ref())
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            fs::rename(temporary, &path).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            File::open(&directory)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            key.as_ref().to_vec()
        }
        Err(e) => return Err(e.to_string()),
    };
    let key = Ed25519KeyPair::from_pkcs8(&bytes)
        .map_err(|_| "Invalid saved guest identity; it was retained.")?;
    let signature = key.sign(&mmorpg_game_server::invitation::admission_message(
        lobby,
        participant,
        matched,
    ));
    let mut proof = key.public_key().as_ref().to_vec();
    proof.extend_from_slice(signature.as_ref());
    Ok(proof)
}
