// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![windows_subsystem = "windows"]
// #![cfg_attr(
    // all(not(debug_assertions), target_os = "windows"),
    // windows_subsystem = "windows"
// )]
#[allow(unused_imports)]
use clap::{Arg, Command};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, BufRead, BufReader};
use serde::{Serialize, Deserialize};
use std::sync::Mutex;
use std::sync::Arc;
use std::ffi::CString;
mod compression;
mod context_menu;
mod file_utils;
mod gui;
use compression::{compress_files, decompress_files, CompressionType, is_compressed_file};
use context_menu::ContextMenuManager;
use tauri::{Manager, AppHandle};

#[derive(Serialize, Deserialize, Debug)]
struct FileCollectionSession {
    timestamp: u64,
    files: Vec<PathBuf>,
    operation: String, // "compress" or "decompress"
}

const COLLECTION_TIMEOUT_MS: u64 = 500; // Wait 500ms for more files
const SESSION_FILE_PREFIX: &str = "tauzip_session_";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let ars = std::env::args().into_iter().collect::<Vec<String>>();
	if ars.len() > 2 && ars[1].to_string().to_lowercase() == "gui-compress".to_string() {
		let args: Vec<String> = std::env::args().into_iter().skip(2).collect::<Vec<String>>();
	
		gui::run_compression_dialog(args, vec![]).await?;
		return Ok(());
	} else if ars.len() > 2 && ars[1].to_string().to_lowercase() == "gui-decompress".to_string() {
		let args: Vec<String> = std::env::args().into_iter().skip(2).collect::<Vec<String>>();
	
		gui::run_decompression_dialog(args, vec![]).await?;
		return Ok(());
	}
	
    let matches = Command::new("tauzip")
        .version("0.1.0")
        .about("Cross-platform compression utility with context menu integration")
        .subcommand(
            Command::new("install")
                .about("Install context menu integration")
        )
        .subcommand(
            Command::new("uninstall")
                .about("Remove context menu integration")
        )
        .subcommand(
            Command::new("compress")
                .about("Compress files")
                .arg(Arg::new("files")
                    .help("Files to compress")
                    .required(true)
                    .num_args(1..)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .subcommand(
            Command::new("decompress")
                .about("Decompress files (command line, no GUI)")
                .arg(Arg::new("files")
                    .help("Files to decompress")
                    .required(true)
                    .num_args(1..)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .subcommand(
            Command::new("decompress-here")
                .about("Decompress files to current directory (command line)")
                .arg(Arg::new("directory")
                    .help("Directory to decompress archives in")
                    .required(true)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .subcommand(
            Command::new("gui-compress")
                .about("Show compression GUI")
                .arg(Arg::new("files")
                    .help("Files to compress")
                    .required(true)
                    .num_args(1..)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .subcommand(
            Command::new("gui-decompress")
                .about("Show decompression GUI with progress")
                .arg(Arg::new("files")
                    .help("Files to decompress")
                    .required(true)
                    .num_args(1..)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .subcommand(
			Command::new("gui-compress-multiple")
				.about("Handle multiple file compression (with aggregation)")
				.arg(Arg::new("files")
					.help("Files to compress (multiple files from %V)")
					.required(true)
					.num_args(1..) // Accept 1 or more arguments
					.value_parser(clap::value_parser!(PathBuf)))
		)
		.subcommand(
			Command::new("gui-decompress-multiple")
				.about("Handle multiple file decompression (with aggregation)")
				.arg(Arg::new("files")
					.help("Files to decompress (multiple files from %V)")
					.required(true)
					.num_args(1..)
					.value_parser(clap::value_parser!(PathBuf)))
		)
        .subcommand(
            Command::new("gui-compress-selection")
                .about("Compress currently selected files in Explorer")
        )
        .subcommand(
            Command::new("gui-decompress-here")
                .about("Show decompression GUI for directory archives")
                .arg(Arg::new("directory")
                    .help("Directory to decompress archives in")
                    .required(true)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .subcommand(
            Command::new("test")
                .about("Test context menu integration")
        )
        .subcommand(
            Command::new("test-gui")
                .about("Test GUI with sample files")
        )
        .subcommand(
            Command::new("debug")
                .about("Debug Tauri setup and file paths")
                .arg(Arg::new("files")
                    .help("Test files")
                    .num_args(0..)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .subcommand(
            Command::new("open")
                .about("Test opening file location")
                .arg(Arg::new("file")
                    .help("File path to open")
                    .required(true)
                    .value_parser(clap::value_parser!(PathBuf)))
        )
        .get_matches();

    match matches.subcommand() {
        Some(("install", _)) => {
            let menu_manager = ContextMenuManager::new();
            menu_manager.install().await?;
            println!("Context menu integration installed successfully!");
            println!("You should now see tauzip options when you right-click on files and folders:");
            println!("• Right-click any file/folder: 'tauzip' submenu with 'Compress' and 'Decompress'");
            println!("• Multiple file selection will now be handled properly!");
            println!("• Decompression shows progress bar and supports cancellation");
        },
        Some(("uninstall", _)) => {
            let menu_manager = ContextMenuManager::new();
            menu_manager.uninstall().await?;
            println!("Context menu integration removed successfully!");
        },
        Some(("compress", sub_matches)) => {
            let files: Vec<PathBuf> = sub_matches.get_many::<PathBuf>("files")
                .unwrap()
                .cloned()
                .collect();
				
            // For CLI compression, default to zip
            let output_path = generate_output_path(&files, CompressionType::Zip);
            compress_files(&files, &output_path, CompressionType::Zip).await?;
            println!("Files compressed to: {}", output_path.display());
        },
        Some(("decompress", sub_matches)) => {
            let files: Vec<PathBuf> = sub_matches.get_many::<PathBuf>("files")
                .unwrap()
                .cloned()
                .collect();
            
            for file in files {
                if !file.exists() {
                    eprintln!("Error: File does not exist: {}", file.display());
                    continue;
                }
                
                if !is_compressed_file(&file) {
                    eprintln!("Error: '{}' is not a supported archive format.", file.display());
                    eprintln!("Supported formats: .zip, .rar, .gz, .bz2, .tar, .7z, .gzip, .br, .tgz, .tar.gz, .tar.br");
                    continue;
                }
                
                let output_dir = generate_output_dir(&file);
                match decompress_files(&file, &output_dir).await {
                    Ok(_) => println!("File decompressed to: {}", output_dir.display()),
                    Err(e) => eprintln!("Failed to decompress '{}': {}", file.display(), e),
                }
            }
        },
        Some(("decompress-here", sub_matches)) => {
            let directory: PathBuf = sub_matches.get_one::<PathBuf>("directory")
                .unwrap()
                .clone();
            
            if !directory.is_dir() {
                eprintln!("Error: '{}' is not a directory", directory.display());
                return Ok(());
            }
            
            println!("Looking for archives in: {}", directory.display());
            
            // Find all archive files in the directory
            let mut archive_files = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && is_compressed_file(&path) {
                        archive_files.push(path);
                    }
                }
            }
			let mut files2 = archive_files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
            
            if archive_files.is_empty() {
                println!("No supported archive files found in the directory.");
                println!("Supported formats: .zip, .rar, .gz, .bz2, .tar, .7z, .gzip, .br, .tgz, .tar.gz, .tar.br");
                return Ok(());
            }
            
            println!("Found {} archive file(s):", archive_files.len());
            for (i, file) in archive_files.iter().enumerate() {
                println!("  {}: {}", i + 1, file.file_name().unwrap_or_default().to_string_lossy());
            }
            
            // Extract each archive
            for file in archive_files {
                let output_dir = generate_output_dir(&file);
                match decompress_files(&file, &output_dir).await {
                    Ok(_) => println!("✓ Extracted: {} -> {}", 
                        file.file_name().unwrap_or_default().to_string_lossy(),
                        output_dir.display()),
                    Err(e) => eprintln!("✗ Failed to extract '{}': {}", 
                        file.file_name().unwrap_or_default().to_string_lossy(), e),
                }
            }
        },
        Some(("gui-compress", sub_matches)) => {
            let files: Vec<PathBuf> = sub_matches.get_many::<PathBuf>("files")
                .unwrap()
                .cloned()
                .collect();
				
			let mut files2 = files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
            
            let x = gui::run_compression_dialog(files2, files).await?;
			 
        },
        Some(("gui-decompress", sub_matches)) => {
            let files: Vec<PathBuf> = sub_matches.get_many::<PathBuf>("files")
                .unwrap()
                .cloned()
                .collect();
			            
            // Filter to only include valid archive files
            let archive_files: Vec<PathBuf> = files.into_iter()
                .filter(|file| {
                    if !file.exists() {
                        eprintln!("Warning: File does not exist: {}", file.display());
                        false
                    } else if !is_compressed_file(file) {
                        eprintln!("Warning: '{}' is not a supported archive format.", file.display());
                        false
                    } else {
                        true
                    }
                })
                .collect();
				
            let mut files2 = archive_files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
			
            // if archive_files.is_empty() {
                // eprintln!("Error: No valid archive files found.");
                // eprintln!("Supported formats: .zip, .rar, .gz, .bz2, .tar, .7z, .gzip, .br, .tgz, .tar.gz, .tar.br");
                // return Ok(());
            // }
            
            let x = gui::run_decompression_dialog(files2.clone(), archive_files).await?;
			 
        },
		Some(("gui-compress-multiple", sub_matches)) => {
			let files: Vec<PathBuf> = sub_matches.get_many::<PathBuf>("files")
				.unwrap()
				.cloned()
				.collect();
				
			let mut files2 = files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
			
			// println!("GUI Compress Multiple called with {} files:", files.len());
			// for (i, file) in files.iter().enumerate() {
				// println!("  {}: {}", i + 1, file.display());
			// }
			
			// if files.is_empty() {
				// println!("Warning: No files provided!");
				// return Ok(());
			// }
			
			// For multiple files from %V, we can skip the aggregation logic
			// since Windows already collected them for us
			let x = gui::run_compression_dialog(files2, files).await?;
		},
		Some(("gui-decompress-multiple", sub_matches)) => {
			let files: Vec<PathBuf> = sub_matches.get_many::<PathBuf>("files")
				.unwrap()
				.cloned()
				.collect();
			
			// println!("GUI Decompress Multiple called with {} files:", files.len());
			// for (i, file) in files.iter().enumerate() {
				// println!("  {}: {}", i + 1, file.display());
			// }
			
			// if files.is_empty() {
				// println!("Warning: No files provided!");
				// return Ok(());
			// }
			
			// Filter to only include valid archive files
			let archive_files: Vec<PathBuf> = files.into_iter()
				.filter(|file| {
					if !file.exists() {
						eprintln!("Warning: File does not exist: {}", file.display());
						false
					} else if !is_compressed_file(file) {
						eprintln!("Warning: '{}' is not a supported archive format.", file.display());
						false
					} else {
						true
					}
				})
				.collect();
				
			let mut files2 = archive_files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
			
			if archive_files.is_empty() {
				eprintln!("Error: No valid archive files found.");
				eprintln!("Supported formats: .zip, .rar, .gz, .bz2, .tar, .7z, .gzip, .br, .tgz, .tar.gz, .tar.br");
				return Ok(());
			}
			
			let x = gui::run_decompression_dialog(files2, archive_files).await?;
		},
        Some(("gui-compress-selection", _)) => {
            println!("GUI Compress Selection - attempting to get selected files from Explorer...");
            
            // Try to get selected files from Windows Explorer using PowerShell
            #[cfg(target_os = "windows")]
            {
                let selected_files = get_selected_files_windows().await?;
                if !selected_files.is_empty() {
                    println!("Found {} selected files:", selected_files.len());
                    for (i, file) in selected_files.iter().enumerate() {
                        println!("  {}: {}", i + 1, file.display());
                    }
					let mut files2 = selected_files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
                    let x = gui::run_compression_dialog(files2, selected_files).await?;
					 
                } else {
                    eprintln!("No files are currently selected in Explorer.");
                }
            }
            
            #[cfg(not(target_os = "windows"))]
            {
                eprintln!("Selection compression is currently only supported on Windows.");
            }
        },
        Some(("gui-decompress-here", sub_matches)) => {
            let directory: PathBuf = sub_matches.get_one::<PathBuf>("directory")
                .unwrap()
                .clone();
            
            if !directory.is_dir() {
                eprintln!("Error: '{}' is not a directory", directory.display());
                return Ok(());
            }
            
            println!("Looking for archives in: {}", directory.display());
            
            // Find all archive files in the directory
            let mut archive_files = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && is_compressed_file(&path) {
                        archive_files.push(path);
                    }
                }
            }
			let mut files2 = archive_files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
            
            if archive_files.is_empty() {
                eprintln!("Error: No supported archive files found in the directory.");
                eprintln!("Supported formats: .zip, .rar, .gz, .bz2, .tar, .7z, .gzip, .br, .tgz, .tar.gz, .tar.br");
                return Ok(());
            }
            
            println!("Found {} archive file(s) to decompress.", archive_files.len());
            let x = gui::run_decompression_dialog(files2, archive_files).await?;
			 
        },
        Some(("test", _)) => {
            println!("Testing context menu integration...");
            println!("Executable path: {}", std::env::current_exe()?.display());
            
            #[cfg(target_os = "windows")]
            {
                use winreg::{enums::*, RegKey};
                let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
                
                println!("\nChecking Windows registry entries:");
                let entries_to_check = [
                    "*\\shell\\tauzip", 
                    "*\\shell\\tauzip\\shell\\compress",
                    "*\\shell\\tauzip\\shell\\decompress",
                    "Directory\\shell\\tauzip",
                    "Directory\\shell\\tauzip\\shell\\compress",
                    "Directory\\Background\\shell\\tauzip",
                ];

                for entry in &entries_to_check {
                    match hkcr.open_subkey(entry) {
                        Ok(_) => println!("✓ Found: {}", entry),
                        Err(_) => println!("✗ Missing: {}", entry),
                    }
                }
            }
            
            println!("\nExpected context menu items:");
            println!("- Right-click files/folders: 'tauzip' submenu with 'Compress' and 'Decompress'");
            println!("- Multiple file selection will be properly aggregated into a single dialog");
            println!("- Background right-click: 'TauZip - Compress Selected' for current selection");
            println!("\nNote: Multi-file selection is now properly supported!");
            println!("When you select multiple files and right-click, they will all appear in one dialog.");
            println!("\nIf context menu items are not showing:");
            println!("1. Make sure you ran as Administrator on Windows");
            println!("2. Try logging out and back in");
            println!("3. Try running: tauzip uninstall && tauzip install");
            println!("4. Check if the executable path is correct");
        },
        Some(("test-gui", _)) => {
            println!("Testing GUI with sample files...");
            let sample_files = vec![
                std::env::current_exe()?,  // Use the executable itself as a test file
            ];
			let files2 = vec![std::env::current_exe()?.display().to_string()];
            let x = gui::run_compression_dialog(files2, sample_files).await?;
			 
        },
        Some(("debug", sub_matches)) => {
            println!("=== tauzip Debug Information ===");
            println!("Executable: {}", std::env::current_exe()?.display());
            println!("Current dir: {}", std::env::current_dir()?.display());
            
            let test_files: Vec<PathBuf> = if let Some(files) = sub_matches.get_many::<PathBuf>("files") {
                files.cloned().collect()
            } else {
                vec![std::env::current_exe()?]  // Use executable as test file
            };
            
            println!("Test files ({}):", test_files.len());
            for (i, file) in test_files.iter().enumerate() {
                println!("  {}: {} (exists: {})", i + 1, file.display(), file.exists());
            }
			let mut files2 = test_files.iter().map(|x| x.display().to_string()).collect::<Vec<String>>();
            
            println!("\nChecking dist directory:");
            let dist_path = std::env::current_exe()?.parent().unwrap().join("../dist");
            let dist_index = dist_path.join("index.html");
            println!("  dist/index.html: {} (exists: {})", dist_index.display(), dist_index.exists());
            
            // Test file aggregation system
            println!("\nTesting file aggregation system...");
            let temp_dir = std::env::temp_dir();
            println!("Temp directory: {}", temp_dir.display());
            
            // Clean up any old session files
            cleanup_old_sessions().await?;
            
            println!("\nTesting GUI with sample files...");
            let x = gui::run_compression_dialog(files2, test_files).await?;
			 
        },
        Some(("open", sub_matches)) => {
            let file_path: PathBuf = sub_matches.get_one::<PathBuf>("file").unwrap().clone();
            println!("Testing file location opening for: {}", file_path.display());
            
            #[cfg(target_os = "windows")]
            {
                println!("Running: explorer /select,\"{}\"", file_path.display());
                match std::process::Command::new("explorer")
                    .arg("/select,")
                    .arg(&file_path)
                    .spawn()
                {
                    Ok(_) => println!("✓ Successfully opened file location"),
                    Err(e) => println!("✗ Failed to open explorer: {}", e),
                }
            }
            
            #[cfg(target_os = "macos")]
            {
                println!("Running: open -R \"{}\"", file_path.display());
                match std::process::Command::new("open")
                    .arg("-R")
                    .arg(&file_path)
                    .spawn()
                {
                    Ok(_) => println!("✓ Successfully opened file location"),
                    Err(e) => println!("✗ Failed to open finder: {}", e),
                }
            }
            
            #[cfg(target_os = "linux")]
            {
                let parent_dir = file_path.parent().unwrap_or_else(|| std::path::Path::new("."));
                println!("Opening directory: {}", parent_dir.display());
                
                let file_managers = ["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"];
                let mut opened = false;
                
                for fm in &file_managers {
                    println!("Trying: {} \"{}\"", fm, parent_dir.display());
                    if let Ok(_) = std::process::Command::new(fm)
                        .arg(parent_dir)
                        .spawn()
                    {
                        println!("✓ Successfully opened with {}", fm);
                        opened = true;
                        break;
                    }
                }
                
                if !opened {
                    println!("✗ No supported file manager found");
                }
            }
        },
        _ => {
            println!("Use --help for available commands");
        }
    }

    Ok(())
}

// async fn handle_file_aggregation(file: PathBuf, operation: &str) -> anyhow::Result<Option<Vec<PathBuf>>> {
    // let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    // let temp_dir = std::env::temp_dir();
    
    // // Clean up old sessions first
    // cleanup_old_sessions().await?;
    
    // // Look for existing session within the timeout window
    // let mut existing_session_file = None;
    // let mut existing_session = None;
    
    // if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        // for entry in entries.flatten() {
            // let path = entry.path();
            // if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                // if filename.starts_with(SESSION_FILE_PREFIX) && filename.ends_with(&format!("_{}.json", operation)) {
                    // if let Ok(file_content) = std::fs::read_to_string(&path) {
                        // if let Ok(session) = serde_json::from_str::<FileCollectionSession>(&file_content) {
                            // if current_time - session.timestamp <= COLLECTION_TIMEOUT_MS {
                                // existing_session_file = Some(path);
                                // existing_session = Some(session);
                                // break;
                            // }
                        // }
                    // }
                // }
            // }
        // }
    // }
    
    // if let Some(mut session) = existing_session {
        // // Add this file to existing session
        // session.files.push(file);
        // session.timestamp = current_time;
        
        // // Write back the updated session
        // if let Some(session_file) = existing_session_file {
            // let updated_content = serde_json::to_string(&session)?;
            // std::fs::write(session_file, updated_content)?;
        // }
        
        // // This instance should exit - another instance will handle the GUI
        // return Ok(None);
    // } else {
        // // Create new session
        // let session = FileCollectionSession {
            // timestamp: current_time,
            // files: vec![file],
            // operation: operation.to_string(),
        // };
        
        // let session_filename = format!("{}{}_{}_{}.json", SESSION_FILE_PREFIX, current_time, std::process::id(), operation);
        // let session_path = temp_dir.join(session_filename);
        
        // let session_content = serde_json::to_string(&session)?;
        // std::fs::write(&session_path, session_content)?;
        
        // // Wait for the timeout period to collect more files
        // //tokio::time::sleep(Duration::from_millis(COLLECTION_TIMEOUT_MS)).await;
        
        // // Read the final session
        // let final_content = std::fs::read_to_string(&session_path)?;
        // let final_session: FileCollectionSession = serde_json::from_str(&final_content)?;
        
        // // Clean up the session file
        // let _ = std::fs::remove_file(&session_path);
        
        // return Ok(Some(final_session.files));
    // }
// }

async fn cleanup_old_sessions() -> anyhow::Result<()> {
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let temp_dir = std::env::temp_dir();
    let max_age_ms = COLLECTION_TIMEOUT_MS * 4; // Clean up sessions older than 2 seconds
    
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                if filename.starts_with(SESSION_FILE_PREFIX) {
                    // Try to extract timestamp from filename
                    if let Some(timestamp_str) = filename.split('_').nth(2) {
                        if let Ok(timestamp) = timestamp_str.parse::<u64>() {
                            if current_time - timestamp > max_age_ms {
                                let _ = std::fs::remove_file(&path);
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(())
}

#[cfg(target_os = "windows")]
async fn get_selected_files_windows() -> anyhow::Result<Vec<PathBuf>> {
    use std::process::Command;
    
    // Use PowerShell to get selected files from Explorer
    let output = Command::new("powershell")
        .args(&[
            "-Command",
            r#"
            Add-Type -AssemblyName System.Windows.Forms
            $shell = New-Object -ComObject Shell.Application
            $windows = $shell.Windows()
            foreach ($window in $windows) {
                if ($window.Name -eq "File Explorer" -or $window.Name -eq "Windows Explorer") {
                    $selection = $window.Document.SelectedItems()
                    foreach ($item in $selection) {
                        Write-Output $item.Path
                    }
                }
            }
            "#
        ])
        .output()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    let files: Vec<PathBuf> = output_str
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .filter(|path| path.exists())
        .collect();
    
    Ok(files)
}

fn generate_output_path(files: &[PathBuf], compression_type: CompressionType) -> PathBuf {
    let base_name = if files.len() == 1 {
        files[0].file_stem().unwrap_or_default().to_string_lossy()
    } else {
        "archive".into()
    };
    
    let parent = files[0].parent().unwrap_or_else(|| std::path::Path::new("."));
    parent.join(format!("{}{}", base_name, compression_type.extension()))
}

fn generate_output_dir(file: &PathBuf) -> PathBuf {
    let base_name = file.file_stem().unwrap_or_default().to_string_lossy();
    let parent = file.parent().unwrap_or_else(|| std::path::Path::new("."));
    
    let mut counter = 1;
    let mut output_dir = parent.join(base_name.as_ref());
    
    while output_dir.exists() {
        counter += 1;
        output_dir = parent.join(format!("{} ({})", base_name, counter));
    }
    
    output_dir
}