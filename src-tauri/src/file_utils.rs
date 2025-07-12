use std::path::Path;

pub fn preserve_file_attributes(source: &Path, _target: &Path) -> std::io::Result<()> {
    let metadata = std::fs::metadata(source)?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = metadata.permissions();
        std::fs::set_permissions(_target, permissions)?;
    }
    
    // Preserve timestamps
    let _accessed = metadata.accessed()?;
    let _modified = metadata.modified()?;
    
    // Note: Setting file times requires platform-specific code
    // This is a simplified version
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // You would use libc calls here to set the timestamps
        // This is beyond the scope of this example
    }
    
    Ok(())
}

pub fn get_unique_name(base_path: &Path) -> std::path::PathBuf {
    let mut counter = 1;
    let mut unique_path = base_path.to_path_buf();
    
    while unique_path.exists() {
        counter += 1;
        if let Some(stem) = base_path.file_stem() {
            if let Some(extension) = base_path.extension() {
                unique_path = base_path.parent().unwrap_or(Path::new("."))
                    .join(format!("{} ({}).{}", 
                        stem.to_string_lossy(), 
                        counter, 
                        extension.to_string_lossy()));
            } else {
                unique_path = base_path.parent().unwrap_or(Path::new("."))
                    .join(format!("{} ({})", stem.to_string_lossy(), counter));
            }
        }
    }
    
    unique_path
}