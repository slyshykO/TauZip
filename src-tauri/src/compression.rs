use anyhow::{Context, Result};
use flate2::{write::GzEncoder, Compression as FlateCompression, GzBuilder};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write, Read};
use std::path::{Path, PathBuf};
use tar::Builder as TarBuilder;
use zip::{write::FileOptions, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionType {
    Zip,
    TarGz,
    TarBr,
    Gz,
    Br,
    Gzip,
    Bzip2,
}

impl CompressionType {
    pub fn extension(&self) -> &'static str {
        match self {
            CompressionType::Zip => ".zip",
            CompressionType::TarGz => ".tar.gz",
            CompressionType::TarBr => ".tar.br",
            CompressionType::Gz => ".gz",
            CompressionType::Br => ".br",
            CompressionType::Gzip => ".gzip",
            CompressionType::Bzip2 => ".bz2",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            ".zip" => Some(CompressionType::Zip),
            ".tar.gz" | ".tgz" => Some(CompressionType::TarGz),
            ".tar.br" => Some(CompressionType::TarBr),
            ".gz" => Some(CompressionType::Gz),
            ".br" => Some(CompressionType::Br),
            ".gzip" => Some(CompressionType::Gzip),
            ".bz2" | ".bzip2" => Some(CompressionType::Bzip2),
            _ => None,
        }
    }

    pub fn supports_multiple_files(&self) -> bool {
        match self {
            CompressionType::Zip | CompressionType::TarGz | CompressionType::TarBr => true,
            CompressionType::Gz | CompressionType::Br | CompressionType::Gzip | CompressionType::Bzip2 => false,
        }
    }
}

pub async fn compress_files(
    files: &[PathBuf],
    output_path: &Path,
    compression_type: CompressionType,
) -> Result<()> {
    compress_files_with_progress(files, output_path, compression_type, |_, _| {}).await
}

pub async fn compress_files_with_progress<F>(
    files: &[PathBuf],
    output_path: &Path,
    compression_type: CompressionType,
    mut progress_callback: F,
) -> Result<()>
where
    F: FnMut(f64, String),
{
    if !compression_type.supports_multiple_files() && files.len() > 1 {
        return Err(anyhow::anyhow!(
            "Compression type {:?} does not support multiple files",
            compression_type
        ));
    }

    match compression_type {
        CompressionType::Zip => compress_zip_with_progress(files, output_path, progress_callback).await,
        CompressionType::TarGz => compress_tar_gz_with_progress(files, output_path, progress_callback).await,
        CompressionType::TarBr => compress_tar_br_with_progress(files, output_path, progress_callback).await,
        CompressionType::Gz | CompressionType::Gzip => {
            let filename = files[0].file_name().unwrap_or_default().to_string_lossy().to_string();
            compress_gz_with_progress(&files[0], output_path, move |progress| {
                progress_callback(progress, filename.clone())
            }).await
        },
        CompressionType::Br => {
            let filename = files[0].file_name().unwrap_or_default().to_string_lossy().to_string();
            compress_br_with_progress(&files[0], output_path, move |progress| {
                progress_callback(progress, filename.clone())
            }).await
        },
        CompressionType::Bzip2 => {
            let filename = files[0].file_name().unwrap_or_default().to_string_lossy().to_string();
            compress_bzip2_with_progress(&files[0], output_path, move |progress| {
                progress_callback(progress, filename.clone())
            }).await
        },
    }
}

async fn compress_zip_with_progress<F>(files: &[PathBuf], output_path: &Path, mut progress_callback: F) -> Result<()>
where
    F: FnMut(f64, String),
{
    let file = File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
    
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // Calculate the common base directory for all files
    let base_dir = if files.len() == 1 {
        // For a single file, use its parent directory
        files[0].parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
    } else {
        // For multiple files, find the common parent directory
        find_common_base_dir(files).unwrap_or_else(|| PathBuf::from("."))
    };

    println!("Using base directory: {}", base_dir.display());

    // Calculate total size for progress tracking
    let total_size = calculate_total_size(files)?;
    let mut processed_size = 0u64;

    for (index, file_path) in files.iter().enumerate() {
        let current_filename = file_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        
        println!("Adding file: {}", file_path.display());
        
        // Update progress before processing each file
        let file_progress = if total_size > 0 {
            (processed_size as f64 / total_size as f64) * 100.0
        } else {
            (index as f64 / files.len() as f64) * 100.0
        };
        progress_callback(file_progress, current_filename.clone());
        
        add_to_zip_with_progress(&mut zip, file_path, &base_dir, &options, &mut processed_size, &mut progress_callback, total_size).await?;
    }

    progress_callback(100.0, "Complete".to_string());
    zip.finish()?;
    Ok(())
}

fn calculate_total_size(files: &[PathBuf]) -> Result<u64> {
    let mut total = 0u64;
    for file_path in files {
        total += calculate_path_size(file_path)?;
    }
    Ok(total)
}

fn calculate_path_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    if path.is_file() {
        total += std::fs::metadata(path)?.len();
    } else if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            total += calculate_path_size(&entry.path())?;
        }
    }
    Ok(total)
}

fn find_common_base_dir(files: &[PathBuf]) -> Option<PathBuf> {
    if files.is_empty() {
        return None;
    }

    // Start with the first file's parent directory
    let mut common_base = files[0].parent()?.to_path_buf();

    // Find the common ancestor of all file paths
    for file in &files[1..] {
        if let Some(file_parent) = file.parent() {
            // Find the common path between current common_base and this file's parent
            if let Some(new_common) = find_common_path(&common_base, file_parent) {
                common_base = new_common;
            } else {
                // No common path found, use current directory
                return Some(PathBuf::from("."));
            }
        }
    }

    Some(common_base)
}

fn find_common_path(path1: &Path, path2: &Path) -> Option<PathBuf> {
    let components1: Vec<_> = path1.components().collect();
    let components2: Vec<_> = path2.components().collect();
    
    let mut common_components = Vec::new();
    
    for (comp1, comp2) in components1.iter().zip(components2.iter()) {
        if comp1 == comp2 {
            common_components.push(comp1.as_os_str());
        } else {
            break;
        }
    }
    
    if common_components.is_empty() {
        None
    } else {
        // Reconstruct the path from components
        let mut result = PathBuf::new();
        for component in common_components {
            result.push(component);
        }
        Some(result)
    }
}

fn add_to_zip_sync_with_progress<F>(
    zip: &mut ZipWriter<BufWriter<File>>,
    file_path: &Path,
    base_dir: &Path,
    options: &FileOptions,
    processed_size: &mut u64,
    progress_callback: &mut F,
    total_size: u64,
) -> Result<()>
where
    F: FnMut(f64, String),
{
    if file_path.is_file() {
        // Calculate relative path from base directory
        let relative_path = if let Ok(rel_path) = file_path.strip_prefix(base_dir) {
            rel_path
        } else {
            // If strip_prefix fails, just use the filename
            Path::new(file_path.file_name().unwrap_or_default())
        };

        let current_filename = file_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        println!("Adding file with relative path: {}", relative_path.display());
        
        // Ensure we use forward slashes for zip paths (cross-platform compatibility)
        let zip_path = relative_path.to_string_lossy().replace('\\', "/");
        
        zip.start_file(&zip_path, *options)?;
        
        let mut file = File::open(file_path)?;
        let bytes_copied = std::io::copy(&mut file, zip)?;
        *processed_size += bytes_copied;
        
        // Update progress after processing this file
        let current_progress = if total_size > 0 {
            (*processed_size as f64 / total_size as f64) * 100.0
        } else {
            100.0
        };
        progress_callback(current_progress, current_filename);
        
    } else if file_path.is_dir() {
        // For directories, recursively add all files
        for entry in std::fs::read_dir(file_path)? {
            let entry = entry?;
            add_to_zip_sync_with_progress(zip, &entry.path(), base_dir, options, processed_size, progress_callback, total_size)?;
        }
    }
    Ok(())
}

async fn add_to_zip_with_progress<F>(
    zip: &mut ZipWriter<BufWriter<File>>,
    file_path: &Path,
    base_dir: &Path,
    options: &FileOptions,
    processed_size: &mut u64,
    progress_callback: &mut F,
    total_size: u64,
) -> Result<()>
where
    F: FnMut(f64, String),
{
    add_to_zip_sync_with_progress(zip, file_path, base_dir, options, processed_size, progress_callback, total_size)
}

async fn compress_tar_gz_with_progress<F>(files: &[PathBuf], output_path: &Path, mut progress_callback: F) -> Result<()>
where
    F: FnMut(f64, String),
{
    let file = File::create(output_path)?;
    let gz_encoder = GzEncoder::new(BufWriter::new(file), FlateCompression::default());
    let mut tar = TarBuilder::new(gz_encoder);

    let total_size = calculate_total_size(files)?;
    let mut processed_size = 0u64;

    for (index, file_path) in files.iter().enumerate() {
        let current_filename = file_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        
        let file_progress = if total_size > 0 {
            (processed_size as f64 / total_size as f64) * 100.0
        } else {
            (index as f64 / files.len() as f64) * 100.0
        };
        progress_callback(file_progress, current_filename.clone());
        
        add_to_tar_with_progress(&mut tar, file_path, &mut processed_size).await?;
    }

    progress_callback(100.0, "Complete".to_string());
    tar.finish()?;
    Ok(())
}

async fn compress_tar_br_with_progress<F>(files: &[PathBuf], output_path: &Path, mut progress_callback: F) -> Result<()>
where
    F: FnMut(f64, String),
{
    let file = File::create(output_path)?;
    let br_encoder = brotli::CompressorWriter::new(BufWriter::new(file), 4096, 6, 22);
    let mut tar = TarBuilder::new(br_encoder);

    let total_size = calculate_total_size(files)?;
    let mut processed_size = 0u64;

    for (index, file_path) in files.iter().enumerate() {
        let current_filename = file_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        
        let file_progress = if total_size > 0 {
            (processed_size as f64 / total_size as f64) * 100.0
        } else {
            (index as f64 / files.len() as f64) * 100.0
        };
        progress_callback(file_progress, current_filename.clone());
        
        add_to_tar_with_progress(&mut tar, file_path, &mut processed_size).await?;
    }

    progress_callback(100.0, "Complete".to_string());
    tar.finish()?;
    Ok(())
}

async fn add_to_tar_with_progress<W: Write>(tar: &mut TarBuilder<W>, file_path: &Path, processed_size: &mut u64) -> Result<()> {
    if file_path.is_file() {
        // For tar files, we can use the filename directly
        let filename = file_path.file_name().unwrap_or_default();
        tar.append_path_with_name(file_path, filename)?;
        
        // Update processed size
        if let Ok(metadata) = std::fs::metadata(file_path) {
            *processed_size += metadata.len();
        }
    } else if file_path.is_dir() {
        tar.append_dir_all(file_path.file_name().unwrap(), file_path)?;
        
        // Update processed size for directory
        if let Ok(size) = calculate_path_size(file_path) {
            *processed_size += size;
        }
    }
    Ok(())
}

// Progress tracking writer wrapper for compression with filename tracking
struct ProgressWriter<W, F> {
    inner: W,
    progress_callback: F,
    total_size: u64,
    bytes_written: u64,
    filename: String,
}

impl<W: Write, F: FnMut(f64)> ProgressWriter<W, F> {
    fn new(inner: W, total_size: u64, filename: String, progress_callback: F) -> Self {
        Self {
            inner,
            progress_callback,
            total_size,
            bytes_written: 0,
            filename,
        }
    }
}

impl<W: Write, F: FnMut(f64)> Write for ProgressWriter<W, F> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let bytes = self.inner.write(buf)?;
        self.bytes_written += bytes as u64;
        
        if self.total_size > 0 {
            let progress = (self.bytes_written as f64 / self.total_size as f64) * 100.0;
            (self.progress_callback)(progress.min(100.0));
        }
        
        Ok(bytes)
    }
    
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

async fn compress_gz_with_progress<F>(file_path: &Path, output_path: &Path, progress_callback: F) -> Result<()>
where
    F: FnMut(f64),
{
    let input = File::open(file_path)?;
    let output = File::create(output_path)?;
    let file_size = std::fs::metadata(file_path)?.len();
    
    let filename = file_path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    
    let progress_output = ProgressWriter::new(output, file_size, filename, progress_callback);
    
    // Create encoder with optional filename in header
    let mut encoder = match file_path.file_name().and_then(|name| name.to_str()) {
        Some(filename_str) => {
            // Store the original filename in the gzip header
            GzBuilder::new()
                .filename(filename_str)
                .write(BufWriter::new(progress_output), FlateCompression::default())
        }
        None => {
            // No filename available, create without header filename
            GzBuilder::new().write(BufWriter::new(progress_output), FlateCompression::default())
        }
    };
    
    let mut reader = BufReader::new(input);
    std::io::copy(&mut reader, &mut encoder)?;
    encoder.finish()?;
    Ok(())
}

async fn compress_br_with_progress<F>(file_path: &Path, output_path: &Path, progress_callback: F) -> Result<()>
where
    F: FnMut(f64),
{
    let input = File::open(file_path)?;
    let output = File::create(output_path)?;
    let file_size = std::fs::metadata(file_path)?.len();
    
    let filename = file_path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    
    let progress_output = ProgressWriter::new(output, file_size, filename, progress_callback);
    let mut encoder = brotli::CompressorWriter::new(BufWriter::new(progress_output), 4096, 6, 22);
    
    let mut reader = BufReader::new(input);
    std::io::copy(&mut reader, &mut encoder)?;
    encoder.flush()?;
    Ok(())
}

async fn compress_bzip2_with_progress<F>(file_path: &Path, output_path: &Path, progress_callback: F) -> Result<()>
where
    F: FnMut(f64),
{
    let input = File::open(file_path)?;
    let output = File::create(output_path)?;
    let file_size = std::fs::metadata(file_path)?.len();
    
    let filename = file_path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    
    let progress_output = ProgressWriter::new(output, file_size, filename, progress_callback);
    let mut encoder = bzip2::write::BzEncoder::new(BufWriter::new(progress_output), bzip2::Compression::default());
    
    let mut reader = BufReader::new(input);
    std::io::copy(&mut reader, &mut encoder)?;
    encoder.finish()?;
    Ok(())
}

// Standard decompression without progress (backward compatibility)
pub async fn decompress_files(file_path: &Path, output_dir: &Path) -> Result<()> {
    decompress_files_with_progress(file_path, output_dir, |_, _| {}).await
}

// New decompression function with filename-aware progress callback
pub async fn decompress_files_with_progress<F>(
    file_path: &Path, 
    output_dir: &Path,
    mut progress_callback: F
) -> Result<()> 
where
    F: FnMut(f64, String),
{
    let extension = file_path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    let file_name = file_path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    // Get file size for progress calculation
    let file_size = std::fs::metadata(file_path)?.len();
    
    let archive_name = file_path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        decompress_tar_gz_with_progress(file_path, output_dir, file_size, move |progress, _| {
            progress_callback(progress, archive_name.clone())
        }).await
    } else if file_name.ends_with(".tar.br") {
        decompress_tar_br_with_progress(file_path, output_dir, file_size, move |progress, _| {
            progress_callback(progress, archive_name.clone())
        }).await
    } else {
        match extension {
            "zip" => decompress_zip_with_progress(file_path, output_dir, move |progress, _| {
                progress_callback(progress, archive_name.clone())
            }).await,
            "gz" | "gzip" => decompress_gz_with_progress(file_path, output_dir, file_size, move |progress, _| {
                progress_callback(progress, archive_name.clone())
            }).await,
            "br" => decompress_br_with_progress(file_path, output_dir, file_size, move |progress, _| {
                progress_callback(progress, archive_name.clone())
            }).await,
            "bz2" | "bzip2" => decompress_bzip2_with_progress(file_path, output_dir, file_size, move |progress, _| {
                progress_callback(progress, archive_name.clone())
            }).await,
            #[cfg(feature = "rar-support")]
            "rar" => decompress_rar(file_path, output_dir).await,
            _ => Err(anyhow::anyhow!("Unsupported file format: {}", extension)),
        }
    }
}

// Progress tracking reader wrapper for decompression with filename tracking
struct ProgressReader<R, F> {
    inner: R,
    progress_callback: F,
    total_size: u64,
    bytes_read: u64,
    filename: String,
}

impl<R: Read, F: FnMut(f64, String)> ProgressReader<R, F> {
    fn new(inner: R, total_size: u64, filename: String, progress_callback: F) -> Self {
        Self {
            inner,
            progress_callback,
            total_size,
            bytes_read: 0,
            filename,
        }
    }
}

impl<R: Read, F: FnMut(f64, String)> Read for ProgressReader<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes = self.inner.read(buf)?;
        self.bytes_read += bytes as u64;
        
        if self.total_size > 0 {
            let progress = (self.bytes_read as f64 / self.total_size as f64) * 100.0;
            (self.progress_callback)(progress.min(100.0), self.filename.clone());
        }
        
        Ok(bytes)
    }
}

async fn decompress_zip_with_progress<F>(
    file_path: &Path, 
    output_dir: &Path, 
    mut progress_callback: F
) -> Result<()>
where
    F: FnMut(f64, String),
{
    let file = File::open(file_path)?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))?;
    let archive_name = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    
    std::fs::create_dir_all(output_dir)?;

    let total_files = archive.len();
    
    for i in 0..archive.len() {
        // Update progress based on file count
        let progress = (i as f64 / total_files as f64) * 100.0;
        progress_callback(progress, archive_name.clone());
        
        let mut file = archive.by_index(i)?;
        let outpath = output_dir.join(file.name());

        if file.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        // Set file permissions if available
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode))?;
        }
    }

    progress_callback(100.0, archive_name);
    Ok(())
}

async fn decompress_tar_gz_with_progress<F>(
    file_path: &Path, 
    output_dir: &Path, 
    file_size: u64,
    progress_callback: F
) -> Result<()>
where
    F: FnMut(f64, String),
{
    let file = File::open(file_path)?;
    let filename = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let progress_reader = ProgressReader::new(file, file_size, filename, progress_callback);
    let gz_decoder = flate2::read::GzDecoder::new(BufReader::new(progress_reader));
    let mut archive = tar::Archive::new(gz_decoder);
    
    std::fs::create_dir_all(output_dir)?;
    archive.unpack(output_dir)?;
    Ok(())
}

async fn decompress_tar_br_with_progress<F>(
    file_path: &Path, 
    output_dir: &Path, 
    file_size: u64,
    progress_callback: F
) -> Result<()>
where
    F: FnMut(f64, String),
{
    let file = File::open(file_path)?;
    let filename = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let progress_reader = ProgressReader::new(file, file_size, filename, progress_callback);
    let br_decoder = brotli::Decompressor::new(BufReader::new(progress_reader), 4096);
    let mut archive = tar::Archive::new(br_decoder);
    
    std::fs::create_dir_all(output_dir)?;
    archive.unpack(output_dir)?;
    Ok(())
}

async fn decompress_gz_with_progress<F>(
    file_path: &Path, 
    output_dir: &Path, 
    file_size: u64,
    progress_callback: F
) -> Result<()>
where
    F: FnMut(f64, String),
{
    let input = File::open(file_path)?;
    let filename = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let progress_reader = ProgressReader::new(input, file_size, filename, progress_callback);
    let mut decoder = flate2::read::GzDecoder::new(BufReader::new(progress_reader));
    
    std::fs::create_dir_all(output_dir)?;
    
    // Try to get the original filename from the gzip header first
    let output_name = if let Some(filename_bytes) = decoder.header().and_then(|h| h.filename()) {
        // Convert bytes to string and create OsString
        match std::str::from_utf8(filename_bytes) {
            Ok(filename_str) => std::ffi::OsString::from(filename_str),
            Err(_) => {
                // If UTF-8 conversion fails, fall back to the compressed filename
                fallback_filename_from_compressed(file_path)
            }
        }
    } else {
        // If no filename in header, try to infer from the compressed filename
        fallback_filename_from_compressed(file_path)
    };
    
    let output_path = output_dir.join(output_name);
    let mut output = File::create(output_path)?;
    
    std::io::copy(&mut decoder, &mut output)?;
    Ok(())
}

fn fallback_filename_from_compressed(file_path: &Path) -> std::ffi::OsString {
    let filename = file_path.file_name().unwrap_or_default().to_string_lossy();
    
    // Handle different compression formats
    let (base_name, stripped) = if filename.ends_with(".gz") {
        (&filename[..filename.len() - 3], true)
    } else if filename.ends_with(".gzip") {
        (&filename[..filename.len() - 5], true)
    } else if filename.ends_with(".br") {
        (&filename[..filename.len() - 3], true)
    } else if filename.ends_with(".bz2") {
        (&filename[..filename.len() - 4], true)
    } else if filename.ends_with(".bzip2") {
        (&filename[..filename.len() - 6], true)
    } else {
        (filename.as_ref(), false)
    };
    
    if stripped && !base_name.is_empty() {
        // If we stripped a compression extension and there's still a base name
        if base_name.contains('.') {
            // Base name already has an extension, use it as-is
            std::ffi::OsString::from(base_name)
        } else {
            // No extension in base name, assume it's a text file
            // You could make this more sophisticated by analyzing file content
            std::ffi::OsString::from(format!("{}.txt", base_name))
        }
    } else {
        // Fallback to file stem if we couldn't parse the format
        file_path.file_stem().unwrap_or_default().to_os_string()
    }
}

async fn decompress_br_with_progress<F>(
    file_path: &Path, 
    output_dir: &Path, 
    file_size: u64,
    progress_callback: F
) -> Result<()>
where
    F: FnMut(f64, String),
{
    let input = File::open(file_path)?;
    let filename = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let progress_reader = ProgressReader::new(input, file_size, filename, progress_callback);
    let mut decoder = brotli::Decompressor::new(BufReader::new(progress_reader), 4096);
    
    std::fs::create_dir_all(output_dir)?;
    
    // Use improved filename logic
    let output_name = fallback_filename_from_compressed(file_path);
    let output_path = output_dir.join(output_name);
    let mut output = File::create(output_path)?;
    
    std::io::copy(&mut decoder, &mut output)?;
    Ok(())
}

async fn decompress_bzip2_with_progress<F>(
    file_path: &Path, 
    output_dir: &Path, 
    file_size: u64,
    progress_callback: F
) -> Result<()>
where
    F: FnMut(f64, String),
{
    let input = File::open(file_path)?;
    let filename = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let progress_reader = ProgressReader::new(input, file_size, filename, progress_callback);
    let mut decoder = bzip2::read::BzDecoder::new(BufReader::new(progress_reader));
    
    std::fs::create_dir_all(output_dir)?;
    
    // Use improved filename logic
    let output_name = fallback_filename_from_compressed(file_path);
    let output_path = output_dir.join(output_name);
    let mut output = File::create(output_path)?;
    
    std::io::copy(&mut decoder, &mut output)?;
    Ok(())
}

#[cfg(feature = "rar-support")]
async fn decompress_rar(file_path: &Path, output_dir: &Path) -> Result<()> {
    use unrar::Archive;
    
    std::fs::create_dir_all(output_dir)?;
    
    // Convert to owned strings to avoid lifetime issues
    let file_path_str = file_path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid file path"))?
        .to_string();
    let output_dir_str = output_dir.to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid output directory path"))?
        .to_string();
    
    // Handle the unrar specific error types using spawn_blocking for thread safety
    tokio::task::spawn_blocking(move || {
        Archive::new(file_path_str)
            .extract_to(output_dir_str)
            .map_err(|e| anyhow::anyhow!("RAR extraction failed: {:?}", e))?
            .process()
            .map_err(|e| anyhow::anyhow!("RAR processing failed: {:?}", e))?;
        Ok::<(), anyhow::Error>(())
    }).await??;
    
    Ok(())
}

pub fn is_compressed_file(path: &Path) -> bool {
    let file_name = path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") || file_name.ends_with(".tar.br") {
        return true;
    }

    let extension = path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    matches!(extension, "zip" | "gz" | "br" | "gzip" | "bzip2" | "bz2" | "rar")
}