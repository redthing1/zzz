//! progress reporting functionality

use indicatif::{ProgressBar, ProgressStyle};

const PROGRESS_TEMPLATE: &str = "{spinner:.green} [{elapsed_precise}] [{bar:.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} ({eta})";

pub struct Progress {
    bar: Option<ProgressBar>,
}

impl Progress {
    /// create new progress reporter, only shows progress if enabled
    pub fn new(enabled: bool, total_bytes: u64) -> Self {
        let bar = if enabled {
            let pb = ProgressBar::new(total_bytes);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(PROGRESS_TEMPLATE)
                    .expect("invalid progress template")
                    .progress_chars("█▉▊▋▌▍▎▏  ")
            );
            Some(pb)
        } else {
            None
        };
        
        Self { bar }
    }
    
    /// update progress with current bytes processed
    pub fn update(&self, processed_bytes: u64) {
        if let Some(bar) = &self.bar {
            bar.set_position(processed_bytes);
        }
    }
    
    /// finish progress with a message
    pub fn finish_with_message(&self, msg: &str) {
        if let Some(bar) = &self.bar {
            bar.finish_with_message(msg.to_string());
        }
    }
    
    /// finish progress and clear
    pub fn finish(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }
}