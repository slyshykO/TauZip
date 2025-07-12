use anyhow::Result;
use std::path::PathBuf;

pub struct ContextMenuManager {
    executable_path: PathBuf,
}

impl ContextMenuManager {
    pub fn new() -> Self {
        let executable_path = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("tauzip"));
        
        Self { executable_path }
    }

    pub async fn install(&self) -> Result<()> {
        // First, clean up any existing entries to avoid conflicts
        self.uninstall().await?;
        
        #[cfg(target_os = "windows")]
        self.install_windows().await?;
        
        #[cfg(target_os = "macos")]
        self.install_macos().await?;
        
        #[cfg(target_os = "linux")]
        self.install_linux().await?;
        
        Ok(())
    }

    pub async fn uninstall(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        self.uninstall_windows().await?;
        
        #[cfg(target_os = "macos")]
        self.uninstall_macos().await?;
        
        #[cfg(target_os = "linux")]
        self.uninstall_linux().await?;
        
        Ok(())
    }

    #[cfg(target_os = "windows")]
    async fn install_windows(&self) -> Result<()> {
        use winreg::{enums::*, RegKey};

        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        
        // Create direct compress menu item for all files - single instance will handle multiple files
        let compress_direct = hkcr.create_subkey("*\\shell\\tauzip_compress")?;
        compress_direct.0.set_value("", &"TauZip - Compress")?;
        compress_direct.0.set_value("MUIVerb", &"TauZip - Compress")?;
        compress_direct.0.set_value("Icon", &format!("{},0", self.executable_path.display()))?;
        compress_direct.0.set_value("MultiSelectModel", &"Player")?;
        let compress_direct_cmd = hkcr.create_subkey("*\\shell\\tauzip_compress\\command")?;
        // Using regular gui-compress - single instance plugin handles multiple files automatically
        compress_direct_cmd.0.set_value("", &format!("\"{}\" gui-compress \"%V\"", self.executable_path.display()))?;

        // Create GUI decompress menu item for all files
        let decompress_direct = hkcr.create_subkey("*\\shell\\tauzip_decompress")?;
        decompress_direct.0.set_value("", &"TauZip - Decompress")?;
        decompress_direct.0.set_value("MUIVerb", &"TauZip - Decompress")?;
        decompress_direct.0.set_value("Icon", &format!("{},0", self.executable_path.display()))?;
        decompress_direct.0.set_value("MultiSelectModel", &"Player")?;
        let decompress_direct_cmd = hkcr.create_subkey("*\\shell\\tauzip_decompress\\command")?;
        // Using regular gui-decompress - single instance plugin handles multiple files automatically
        decompress_direct_cmd.0.set_value("", &format!("\"{}\" gui-decompress \"%V\"", self.executable_path.display()))?;

        // Create direct compress menu item for directories
        let dir_compress_direct = hkcr.create_subkey("Directory\\shell\\tauzip_compress")?;
        dir_compress_direct.0.set_value("", &"TauzZip - Compress")?;
        dir_compress_direct.0.set_value("MUIVerb", &"TauZip - Compress")?;
        dir_compress_direct.0.set_value("Icon", &format!("{},0", self.executable_path.display()))?;
        dir_compress_direct.0.set_value("MultiSelectModel", &"Player")?;
        let dir_compress_direct_cmd = hkcr.create_subkey("Directory\\shell\\tauzip_compress\\command")?;
        dir_compress_direct_cmd.0.set_value("", &format!("\"{}\" gui-compress \"%V\"", self.executable_path.display()))?;

        // Also add decompress option for directories (for cases like extracting to folder)
        let dir_decompress_direct = hkcr.create_subkey("Directory\\shell\\tauzip_decompress")?;
        dir_decompress_direct.0.set_value("", &"TauZip - Decompress Here")?;
        dir_decompress_direct.0.set_value("MUIVerb", &"TauZip - Decompress Here")?;
        dir_decompress_direct.0.set_value("Icon", &format!("{},0", self.executable_path.display()))?;
        dir_decompress_direct.0.set_value("MultiSelectModel", &"Player")?;
        let dir_decompress_direct_cmd = hkcr.create_subkey("Directory\\shell\\tauzip_decompress\\command")?;
        // Keep gui-decompress-here for directories since it has special logic for finding archives in directories
        dir_decompress_direct_cmd.0.set_value("", &format!("\"{}\" gui-decompress-here \"%V\"", self.executable_path.display()))?;

        println!("Windows context menu installed successfully!");
        println!("You should see 'TauZip - Compress' and 'TauZip - Decompress' options in the right-click menu.");
        println!("Multiple file selection is now properly supported with single instance - only one window opens!");
        println!("Decompression will show a progress bar!");
        
        Ok(())
    }

    #[cfg(target_os = "windows")]
    async fn uninstall_windows(&self) -> Result<()> {
        use winreg::{enums::*, RegKey};

        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        
        // Remove all possible tauzip-related context menu entries
        let entries_to_remove = [
            // Direct menu items
            "*\\shell\\tauzip_compress",
            "*\\shell\\tauzip_decompress", 
            "Directory\\shell\\tauzip_compress",
            "Directory\\shell\\tauzip_decompress",
            
            // Potential parent menu items with subitems (from previous versions)
            "*\\shell\\tauzip",
            "Directory\\shell\\tauzip",
            "Directory\\Background\\shell\\tauzip",
            
            // Alternative naming patterns that might exist
            "*\\shell\\tauZip",
            "Directory\\shell\\TauZip",
            "*\\shell\\TAUZIP",
            "Directory\\shell\\TAUZIP",
            
            // Cascading menu entries
            "*\\shell\\tauzip\\shell\\compress",
            "*\\shell\\tauzip\\shell\\decompress",
            "Directory\\shell\\tauzip\\shell\\compress",
            
            // Legacy entries
            "*\\shellex\\ContextMenuHandlers\\tauzip",
            "Directory\\shellex\\ContextMenuHandlers\\tauzip",
        ];

        for entry in &entries_to_remove {
            match hkcr.delete_subkey_all(entry) {
                Ok(_) => println!("Removed: {}", entry),
                Err(_) => {
                    // Ignore errors for entries that don't exist
                    // println!("Note: Entry not found: {}", entry);
                }
            }
        }
        
        // Also check and remove any entries under specific file extensions
        let file_extensions = [
            ".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".gzip", ".br", ".tgz"
        ];
        
        for ext in &file_extensions {
            let ext_key_path = format!("{}\\shell\\tauzip", ext);
            let _ = hkcr.delete_subkey_all(&ext_key_path);
            
            let ext_compress_path = format!("{}\\shell\\tauzip_compress", ext);
            let _ = hkcr.delete_subkey_all(&ext_compress_path);
            
            let ext_decompress_path = format!("{}\\shell\\tauzip_decompress", ext);
            let _ = hkcr.delete_subkey_all(&ext_decompress_path);
        }
        
        println!("Windows context menu cleanup completed!");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn install_macos(&self) -> Result<()> {
        // Create an Automator service or AppleScript application
        let home_dir = dirs::home_dir().unwrap_or_default();
        let services_dir = home_dir.join("Library/Services");
        std::fs::create_dir_all(&services_dir)?;

        let service_content = format!(r#"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSServices</key>
    <array>
        <dict>
            <key>NSMenuItem</key>
            <dict>
                <key>default</key>
                <string>TauZip - Compress</string>
            </dict>
            <key>NSMessage</key>
            <string>gui-compress</string>
            <key>NSPortName</key>
            <string>tauzip</string>
            <key>NSRequiredContext</key>
            <dict>
                <key>NSApplicationIdentifier</key>
                <string>com.finder</string>
            </dict>
            <key>NSSendFileTypes</key>
            <array>
                <string>public.item</string>
            </array>
        </dict>
        <dict>
            <key>NSMenuItem</key>
            <dict>
                <key>default</key>
                <string>TauZip - Decompress</string>
            </dict>
            <key>NSMessage</key>
            <string>gui-decompress</string>
            <key>NSPortName</key>
            <string>tauzip</string>
            <key>NSRequiredContext</key>
            <dict>
                <key>NSApplicationIdentifier</key>
                <string>com.finder</string>
            </dict>
            <key>NSSendFileTypes</key>
            <array>
                <string>public.archive</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
"#);

        std::fs::write(services_dir.join("tauzip.plist"), service_content)?;
        
        // You would need to create a proper macOS service bundle here
        // This is a simplified implementation
        
        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn uninstall_macos(&self) -> Result<()> {
        let home_dir = dirs::home_dir().unwrap_or_default();
        let services_dir = home_dir.join("Library/Services");
        
        // Remove all possible tauzip service files
        let service_files = [
            "tauzip.plist",
            "TauZip.plist", 
            "TAUZIP.plist",
            "tauzip.workflow",
            "TauZip.workflow",
            "TAUZIP.workflow",
        ];
        
        for file in &service_files {
            let service_path = services_dir.join(file);
            if service_path.exists() {
                std::fs::remove_file(service_path)?;
                println!("Removed: {}", file);
            }
        }
        
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn install_linux(&self) -> Result<()> {
        let home_dir = dirs::home_dir().unwrap_or_default();
        let local_share = home_dir.join(".local/share");
        std::fs::create_dir_all(local_share.join("applications"))?;
        std::fs::create_dir_all(local_share.join("file-manager/actions"))?;

        // Create desktop entry
        let desktop_content = format!(r#"[Desktop Entry]
Type=Application
Name=tauzip
Exec={} gui-compress %F
Icon=application-x-archive
StartupNotify=true
NoDisplay=true
MimeType=application/x-archive;application/zip;application/x-tar;application/x-gzip;application/x-bzip2;application/x-compress;application/x-compressed;application/x-cpio;application/x-deb;application/x-rar;
"#, self.executable_path.display());

        std::fs::write(local_share.join("applications/tauzip.desktop"), desktop_content)?;

        // Create file manager action for compression with single instance support
        let compress_action = format!(r#"[Desktop Entry]
Type=Action
Icon=application-x-archive
Name[en]=TauZip - Compress
Tooltip[en]=Compress files with tauzip (single instance handles multiple files)
Profiles=compress;

[X-Action-Profile compress]
Exec={} gui-compress %F
Name[en]=TauZip - Compress
MimeTypes=all/all;
SelectionCount=>0;
"#, self.executable_path.display());

        std::fs::write(local_share.join("file-manager/actions/tauzip-compress.desktop"), compress_action)?;

        // Create file manager action for decompression with GUI progress and single instance support
        let decompress_action = format!(r#"[Desktop Entry]
Type=Action
Icon=application-x-archive
Name[en]=TauZip - Decompress
Tooltip[en]=Decompress archives with tauzip (shows progress, single instance handles multiple files)
Profiles=decompress;

[X-Action-Profile decompress]
Exec={} gui-decompress %F
Name[en]=TauZip - Decompress
MimeTypes=all/all;
SelectionCount=>0;
"#, self.executable_path.display());

        std::fs::write(local_share.join("file-manager/actions/tauzip-decompress.desktop"), decompress_action)?;

        // Try to update the desktop database to register the new entries
        let _ = std::process::Command::new("update-desktop-database")
            .arg(local_share.join("applications"))
            .output();

        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn uninstall_linux(&self) -> Result<()> {
        let home_dir = dirs::home_dir().unwrap_or_default();
        let local_share = home_dir.join(".local/share");
        
        let files_to_remove = [
            "applications/tauzip.desktop",
            "applications/TauZip.desktop",
            "applications/TAUZIP.desktop",
            "file-manager/actions/tauzip-compress.desktop",
            "file-manager/actions/tauzip-decompress.desktop",
            "file-manager/actions/TauZip-compress.desktop",
            "file-manager/actions/TauZip-decompress.desktop",
            "file-manager/actions/TAUZIP-compress.desktop", 
            "file-manager/actions/TAUZIP-decompress.desktop",
        ];

        for file in &files_to_remove {
            let path = local_share.join(file);
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("Removed: {}", path.display());
            }
        }

        Ok(())
    }
}