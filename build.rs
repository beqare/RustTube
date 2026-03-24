use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=lib");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let source_lib_dir = manifest_dir.join("lib");
    if !source_lib_dir.is_dir() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let profile = env::var("PROFILE").expect("missing PROFILE");

    if let Some(profile_dir) = find_profile_dir(&out_dir, &profile) {
        let target_lib_dir = profile_dir.join("lib");
        if let Err(error) = sync_dir(&source_lib_dir, &target_lib_dir) {
            panic!(
                "failed to copy '{}' to '{}': {error}",
                source_lib_dir.display(),
                target_lib_dir.display()
            );
        }
    }
}

fn find_profile_dir(start: &Path, profile: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(profile))
        .map(Path::to_path_buf)
}

fn sync_dir(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            sync_dir(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
        }
    }

    Ok(())
}
