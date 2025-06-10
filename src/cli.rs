//! command line interface

use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "zzz",
    version,
    about = "zzz: compression multitool",
    long_about = "Create and extract zstd-compressed tar archives with smart file filtering"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    
    /// verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
    
    /// number of threads (0 = auto-detect)
    #[arg(long, global = true, default_value = "0")]
    pub threads: u32,
}

#[derive(Subcommand)]
pub enum Commands {
    /// compress files/directories to .zst
    #[command(alias = "c")]
    Compress {
        /// compression level (1-22)
        #[arg(short, long, default_value = "19")]
        level: i32,
        
        /// output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
        
        /// show progress bar
        #[arg(short = 'P', long)]
        progress: bool,
        
        /// exclude files matching pattern (repeatable)
        #[arg(long)]
        exclude: Vec<String>,
        
        /// disable built-in garbage file filtering
        #[arg(long)]
        no_default_excludes: bool,
        
        /// input file or directory
        input: PathBuf,
    },
    
    /// extract .zst archives
    #[command(alias = "x")]
    Extract {
        /// archive file to extract
        archive: PathBuf,
        
        /// destination directory
        destination: Option<PathBuf>,
        
        /// extract to specific directory
        #[arg(short = 'C', long)]
        directory: Option<PathBuf>,
        
        /// overwrite existing files
        #[arg(long)]
        overwrite: bool,
    },
    
    /// list archive contents
    #[command(alias = "l")]
    List {
        /// archive file to list
        archive: PathBuf,
    },
}

impl Cli {
    /// get output path for compression, defaulting to input + .zst
    pub fn get_output_path(input: &PathBuf, output: Option<PathBuf>) -> PathBuf {
        output.unwrap_or_else(|| {
            let mut path = input.clone();
            if let Some(filename) = path.file_name() {
                let mut new_filename = filename.to_os_string();
                new_filename.push(".zst");
                path.set_file_name(new_filename);
            } else {
                path.set_extension("zst");
            }
            path
        })
    }
    
    /// get extraction directory, defaulting to current directory
    pub fn get_extract_dir(destination: Option<PathBuf>, directory: Option<PathBuf>) -> PathBuf {
        directory
            .or(destination)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}