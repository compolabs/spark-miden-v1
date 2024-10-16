use crate::constants::{ACCOUNTS_DIR, DB_FILE_PATH};
use clap::Parser;
use std::{fs, path::Path};

#[derive(Debug, Clone, Parser)]
#[clap(about = "Initialize the order book")]
pub struct InitCmd {}

impl InitCmd {
    pub fn execute(&self) -> Result<(), String> {
        self.remove_file_if_exists(DB_FILE_PATH)?;
        self.remove_folder_if_exists(ACCOUNTS_DIR)?;
        println!("State successfully initialized.");
        Ok(())
    }

    pub fn remove_file_if_exists(&self, file_path: &str) -> Result<(), String> {
        let path = Path::new(file_path);
        if path.exists() {
            fs::remove_file(path)
                .map_err(|e| format!("Failed to remove file {}: {}", file_path, e))?;
        }
        Ok(())
    }

    fn remove_folder_if_exists(&self, folder_path: &str) -> Result<(), String> {
        let path = Path::new(folder_path);
        if path.exists() && path.is_dir() {
            fs::remove_dir_all(path)
                .map_err(|e| format!("Failed to remove folder {}: {}", folder_path, e))?;
        }
        Ok(())
    }
}
