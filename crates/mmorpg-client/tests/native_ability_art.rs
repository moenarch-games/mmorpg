//! Native consumer rejects changed bytes before embedding approved artwork.
#[path = "../build/warden_icons.rs"]
mod warden_icons;

#[test]
fn selected_package_is_pinned_and_missing_or_changed_art_fails() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/assets/warden-icons");
    warden_icons::validate(&source).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), temporary.path().join(entry.file_name())).unwrap();
    }
    let image = temporary.path().join("warden-1-512.png");
    std::fs::write(&image, b"corrupt").unwrap();
    assert!(
        warden_icons::validate(temporary.path())
            .unwrap_err()
            .contains("approved bytes")
    );
    std::fs::remove_file(image).unwrap();
    assert!(
        warden_icons::validate(temporary.path())
            .unwrap_err()
            .contains("warden-1-512.png")
    );
    std::fs::write(temporary.path().join("producer-manifest.json"), b"{}").unwrap();
    assert!(
        warden_icons::validate(temporary.path())
            .unwrap_err()
            .contains("owner-approved pin")
    );
}
