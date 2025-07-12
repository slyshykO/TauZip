use super::compression::{compress_files, decompress_files_with_progress, CompressionType};
use anyhow::Result;
use std::ffi::c_void;
use std::path::{PathBuf, Path};
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use tauri::{Manager, Wry, App, AppHandle, generate_context, WebviewWindow, Emitter, Runtime, Window, Listener};
use serde::{Serialize, Deserialize};
use tauri_plugin_cli::CliExt;
use serde_json::Value;
use tauri_plugin_shell::ShellExt;

#[derive(Clone, Serialize)]
pub struct CompressionProgressUpdate {
    progress: f64,
    current_file: String,
    total_files: usize,
    current_file_index: usize,
    operation: String, // "compressing" or "extracting"
}

#[tauri::command]
async fn compress_files_command(
    window: tauri::Window,
    files: Vec<String>, 
    outputfile: String, 
    compressiontype: String
) -> Result<String, String> {
    println!("Compression request received - files: {:?}, output: {}, type: {}", 
             files, outputfile, compressiontype);
    
    // Convert string to CompressionType enum
    let compression_enum = match compressiontype.as_str() {
        "Zip" => CompressionType::Zip,
        "TarGz" => CompressionType::TarGz,
        "TarBr" => CompressionType::TarBr,
        "Gz" => CompressionType::Gz,
        "Br" => CompressionType::Br,
        "Gzip" => CompressionType::Gzip,
        "Bzip2" => CompressionType::Bzip2,
        _ => return Err(format!("Unsupported compression type: {}", compressiontype)),
    };
    
    // Convert string paths back to PathBuf
    let file_paths: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(f)).collect();
    
    // Construct the full output path
    let output_path = if std::path::Path::new(&outputfile).is_absolute() {
        // If it's already an absolute path, use it as-is
        PathBuf::from(&outputfile)
    } else {
        // If it's a relative path, use the directory of the first file
        if !file_paths.is_empty() {
            let first_file_dir = file_paths[0]
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            first_file_dir.join(&outputfile)
        } else {
            PathBuf::from(&outputfile)
        }
    };
    
    println!("Output path resolved to: {}", output_path.display());
    
    // Use the new progress version
    use super::compression::compress_files_with_progress;
    
    compress_files_with_progress(&file_paths, &output_path, compression_enum, |progress, current_filename| {
        let progress_update = CompressionProgressUpdate {
            progress,
            current_file: current_filename,
            total_files: file_paths.len(),
            current_file_index: 1,
            operation: "compressing".to_string(),
        };
        let _ = window.app_handle().emit("compression-progress", &progress_update);
    })
    .await
    .map_err(|e| {
        let error_msg = format!("Compression failed: {}", e);
        println!("{}", error_msg);
        error_msg
    })?;
    
    // Final progress update
    let final_progress = CompressionProgressUpdate {
        progress: 100.0,
        current_file: "Complete".to_string(),
        total_files: 1,
        current_file_index: 1,
        operation: "compressing".to_string(),
    };
    let _ = window.emit("compression-progress", &final_progress);
    
    let success_msg = format!("Files compressed successfully to: {}", output_path.display());
    println!("{}", success_msg);
    Ok(success_msg)
}

#[tauri::command]
async fn decompress_files_command(
    window: tauri::Window,
    files: Vec<String>
) -> Result<String, String> {
    println!("Decompression request received - files: {:?}", files);
    
    let file_paths: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(f)).collect();
    let total_files = file_paths.len();
    
    let mut decompressed_to = Vec::new();
    
    for (index, file_path) in file_paths.iter().enumerate() {
        // Generate output directory for this file
        let output_dir = generate_output_dir(file_path);
        
        // Update progress
        let progress = CompressionProgressUpdate {
            progress: (index as f64 / total_files as f64) * 100.0,
            current_file: file_path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            total_files,
            current_file_index: index + 1,
            operation: "extracting".to_string(),
        };
        
        let _ = window.emit("compression-progress", &progress);
        
        // Decompress the file
        match decompress_files_with_progress(file_path, &output_dir, |file_progress, current_filename| {
            // Create a more detailed progress update
            let detailed_progress = CompressionProgressUpdate {
                progress: ((index as f64 + file_progress / 100.0) / total_files as f64) * 100.0,
                current_file: current_filename,
                total_files,
                current_file_index: index + 1,
                operation: "extracting".to_string(),
            };
            let _ = window.emit("compression-progress", &detailed_progress);
        }).await {
            Ok(_) => {
                decompressed_to.push(output_dir.display().to_string());
                println!("File decompressed to: {}", output_dir.display());
            },
            Err(e) => {
                let error_msg = format!("Failed to decompress '{}': {}", file_path.display(), e);
                println!("{}", error_msg);
                return Err(error_msg);
            }
        }
    }
    
    // Final progress update
    let final_progress = CompressionProgressUpdate {
        progress: 100.0,
        current_file: "Complete".to_string(),
        total_files,
        current_file_index: total_files,
        operation: "extracting".to_string(),
    };
    let _ = window.app_handle().emit("compression-progress", &final_progress);
    
    let success_msg = if decompressed_to.len() == 1 {
        format!("File decompressed successfully to: {}", decompressed_to[0])
    } else {
        format!("Files decompressed successfully. {} archives processed.", decompressed_to.len())
    };
    
    println!("{}", success_msg);
    Ok(success_msg)
}

#[tauri::command]
async fn get_compression_types() -> Vec<String> {
    vec![
        "Zip".to_string(),
        "TarGz".to_string(),
        "TarBr".to_string(),
        "Gz".to_string(),
        "Br".to_string(),
        "Gzip".to_string(),
        "Bzip2".to_string(),
    ]
}

#[tauri::command]
async fn validate_compression_type(files: Vec<String>, compressiontype: String) -> Result<bool, String> {
    // Convert string to CompressionType enum
    let compression_enum = match compressiontype.as_str() {
        "Zip" => CompressionType::Zip,
        "TarGz" => CompressionType::TarGz,
        "TarBr" => CompressionType::TarBr,
        "Gz" => CompressionType::Gz,
        "Br" => CompressionType::Br,
        "Gzip" => CompressionType::Gzip,
        "Bzip2" => CompressionType::Bzip2,
        _ => return Err(format!("Unsupported compression type: {}", compressiontype)),
    };
    
    if !compression_enum.supports_multiple_files() && files.len() > 1 {
        return Ok(false);
    }
    Ok(true)
}

#[tauri::command]
fn get_file_args(state: tauri::State<'_, Arc<Mutex<Vec<String>>>>) -> Vec<String> {
    state.lock().unwrap().clone()
}

#[tauri::command]
fn close(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.close(); // or .unwrap() if you want to panic on error
		return Ok(());
    }
	return Err("Unable to close window".to_string());
}

#[tauri::command]
async fn open_file_location(file_path: String) -> Result<(), String> {
    let path = PathBuf::from(&file_path);
    
    println!("Opening file location for: {}", file_path);
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&file_path)
            .spawn()
            .map_err(|e| format!("Failed to open explorer: {}", e))?;
    }
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&file_path)
            .spawn()
            .map_err(|e| format!("Failed to open finder: {}", e))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        let parent_dir = path.parent()
            .ok_or_else(|| "Could not determine parent directory".to_string())?;
        
        // Try different file managers
        let file_managers = ["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"];
        let mut opened = false;
        
        for fm in &file_managers {
            if let Ok(_) = std::process::Command::new(fm)
                .arg(parent_dir)
                .spawn()
            {
                opened = true;
                break;
            }
        }
        
        if !opened {
            return Err("No supported file manager found".to_string());
        }
    }
    
    Ok(())
}

pub fn run_app(app: &AppHandle, file_strings2: Vec<String>, argv: Vec<String>) {
	let log = false;
	if log { std::fs::write("aa.txt", format!("run_app")); }
	
	let mut file_strings3 = file_strings2.clone();
	
	let file_args: Vec<String> = argv.into_iter().skip(2).collect();
	let file_strings4 = file_args.clone();
	let file_paths: Vec<PathBuf> = file_args.iter().map(PathBuf::from).collect();
	
	if log { std::fs::write("aa.txt", format!("file_args {:?}", file_strings4)); }
	
	let appx = Arc::new(Mutex::new(app.clone()));
	tokio::spawn(async move {
		thread::sleep(Duration::from_millis(1500));
		
		let app2 = appx.lock().unwrap();
		let cs = file_strings3.clone();
		for c in cs {
			match app2.emit("files-selected", c.clone()) {
				Ok(_) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("aa {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit success {:?}", s)); }
					println!("Successfully emitted files-selected event with {} files", file_strings3.len())
				},
				Err(e) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("aa {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit fail {:?}", file_strings4)); }
					println!("Failed to emit files-selected event: {}", e)
				},
			}
		}
		
		let cs2 = file_strings4.clone();
		for c in cs2 {
			match app2.emit("files-selected", c.clone()) {
				Ok(_) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("ab {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit success {:?}", s)); }
					println!("Successfully emitted files-selected event with {} files", file_strings3.len())
				},
				Err(e) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("ab {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit fail {:?}", file_strings4)); }
					println!("Failed to emit files-selected event: {}", e)
				},
			}
		}
	});
}

pub fn run_decom_app(app: &AppHandle, file_strings: Vec<String>, argv: Vec<String>) {
	let log = false;
	if log { std::fs::write("aa.txt", "got main decom"); }
		
	let file_strings2 = file_strings.clone();
	let file_strings2b = file_strings2.clone();
		
	let file_strings3 = file_strings2.clone();
	if log { std::fs::write("b.txt", format!("file decom 3 {:?}", &file_strings3)); }
	
	let file_args: Vec<String> = argv.into_iter().skip(2).collect();
	let file_strings4 = file_args.clone();
	let file_paths: Vec<PathBuf> = file_args.iter().map(PathBuf::from).collect();

	let appx = Arc::new(Mutex::new(app.clone()));
	tokio::spawn(async move {
		println!("Setting decompression mode and emitting archives-selected event...");
		thread::sleep(Duration::from_millis(1500));
		
		let app2 = appx.lock().unwrap();
		
		match app2.emit("set-mode", "decompression") {
			Ok(_) => println!("Successfully set decompression mode"),
			Err(e) => println!("Failed to set decompression mode: {}", e),
		}
		let cs = file_strings3.clone();
		for c in cs {
			match app2.emit("archives-selected", c.clone()) {
				Ok(_) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("ba {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit decom success {:?}", s)); }
					println!("Successfully emitted files-selected event with {} files", file_strings3.len())
				},
				Err(e) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("ba {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit decom fail {:?}", file_strings4)); }
					println!("Failed to emit files-selected event: {}", e)
				},
			}
		}
		
		let cs2 = file_strings4.clone();
		for c in cs2 {
			match app2.emit("archives-selected", c.clone()) {
				Ok(_) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("bb {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit decom success {:?}", s)); }
					println!("Successfully emitted files-selected event with {} files", file_strings3.len())
				},
				Err(e) => {
					let s = Path::new(&c);
					if log { std::fs::write(format!("bb {}.txt", s.file_name().unwrap().to_string_lossy()), format!("emit decom fail {:?}", file_strings4)); }
					println!("Failed to emit files-selected event: {}", e)
				},
			}
		}
	});
}

pub async fn run_compression_dialog(file_strings: Vec<String>, files: Vec<PathBuf>) -> Result<()> {
    println!("Starting Tauri compression app with {} files", files.len());
    
	let log = false;
	let file_strings2 = file_strings.clone();
	let file_strings2b = file_strings.clone();
    if log { std::fs::write("a.txt", "before"); }
	
	tauri::Builder::default()
		.invoke_handler(tauri::generate_handler![
            compress_files_command,
            get_compression_types,
            validate_compression_type,
            open_file_location,
			close
        ])
		.plugin(tauri_plugin_shell::init())
		.plugin(tauri_plugin_cli::init())
        .plugin(tauri_plugin_single_instance::init(move |app, argv, _cwd| {
			println!("Tauri compression app setup started");
			if log { std::fs::write("abc.txt", format!("{:?}", argv.clone())); }
            run_app(app, file_strings2.clone(), argv.clone());
			
			//return Ok(());
		}))
		.setup(move |app| {
			if let Some(window) = app.get_webview_window("main") {
				let _ = window.center();
			}
			let mut fb = vec![];
			for x in files {
				fb.push(x.display().to_string());
			}
			run_app(&app.app_handle(), file_strings2b.clone(), fb.clone() );
			
			return Ok(());
		}
		)
		.run(tauri::generate_context!())
        .expect("error while running tauri application");
    
	Ok(())
}

pub async fn run_decompression_dialog(file_strings: Vec<String>, files: Vec<PathBuf>) -> Result<()> {
    println!("Starting Tauri decompression app with {} files", files.len());
    
	let log = false;
    let file_strings2 = file_strings.clone();
	let file_strings2b = file_strings.clone();
	
	tauri::Builder::default()
		.invoke_handler(tauri::generate_handler![
            decompress_files_command,
            open_file_location,
			close
        ])
		.plugin(tauri_plugin_shell::init())
		.plugin(tauri_plugin_cli::init())
        .plugin(tauri_plugin_single_instance::init(move |app, argv, _cwd| {
			run_decom_app(app, file_strings2.clone(), argv.clone());
        }))
		.setup(move |app| {
			if let Some(window) = app.get_webview_window("main") {
				let _ = window.center();
			}
			let mut fb = vec![];
			for x in files {
				fb.push(x.display().to_string());
			}
			run_decom_app(&app.app_handle(), file_strings2b.clone(), fb.clone());
			
			return Ok(());
		}
        )
		.run(tauri::generate_context!())
        .expect("error while running tauri application");
		
	Ok(())
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