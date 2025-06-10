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
                    .progress_chars("█▉▊▋▌▍▎▏  "),
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

    /// set the current position
    pub fn set_position(&self, pos: u64) {
        if let Some(bar) = &self.bar {
            bar.set_position(pos);
        }
    }

    /// set the total length
    pub fn set_length(&self, len: u64) {
        if let Some(bar) = &self.bar {
            bar.set_length(len);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_disabled() {
        let progress = Progress::new(false, 1000);

        // Should not panic when progress is disabled
        progress.update(500);
        progress.finish_with_message("test");
        progress.finish();

        // Progress bar should be None when disabled
        assert!(progress.bar.is_none());
    }

    #[test]
    fn test_progress_enabled() {
        let progress = Progress::new(true, 1000);

        // Progress bar should exist when enabled
        assert!(progress.bar.is_some());

        // Should not panic with progress operations
        progress.update(500);
        progress.update(750);
        progress.finish_with_message("completed");
    }

    #[test]
    fn test_progress_zero_total() {
        let progress = Progress::new(true, 0);

        // Should handle zero total bytes without panic
        progress.update(0);
        progress.finish();
    }

    #[test]
    fn test_progress_update_beyond_total() {
        let progress = Progress::new(true, 100);

        // Should handle updates beyond total without panic
        progress.update(150);
        progress.finish();
    }
}
