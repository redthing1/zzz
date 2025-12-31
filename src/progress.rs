//! progress reporting functionality

use indicatif::{ProgressBar, ProgressStyle};
use std::io::Read;

const PROGRESS_BYTES_TEMPLATE: &str =
    "{spinner:.green} [{elapsed_precise}] [{bar:.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} ({eta})";
const PROGRESS_ITEMS_TEMPLATE: &str =
    "{spinner:.green} [{elapsed_precise}] [{bar:.cyan/blue}] {pos}/{len} items ({eta})";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressKind {
    Bytes,
    Items,
}

pub struct Progress {
    bar: Option<ProgressBar>,
    verbose: bool,
    kind: ProgressKind,
}

impl Progress {
    fn new_with_template(
        enabled: bool,
        total: u64,
        template: &str,
        verbose: bool,
        kind: ProgressKind,
    ) -> Self {
        let bar = if enabled {
            let pb = ProgressBar::new(total);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(template)
                    .expect("invalid progress template")
                    .progress_chars("█▉▊▋▌▍▎▏  "),
            );
            Some(pb)
        } else {
            None
        };

        Self { bar, verbose, kind }
    }

    /// create new byte-based progress reporter, only shows progress if enabled
    pub fn new(enabled: bool, total_bytes: u64, verbose: bool) -> Self {
        Self::new_with_template(
            enabled,
            total_bytes,
            PROGRESS_BYTES_TEMPLATE,
            verbose,
            ProgressKind::Bytes,
        )
    }

    /// create new item-count progress reporter, only shows progress if enabled
    pub fn new_items(enabled: bool, total_items: u64, verbose: bool) -> Self {
        Self::new_with_template(
            enabled,
            total_items,
            PROGRESS_ITEMS_TEMPLATE,
            verbose,
            ProgressKind::Items,
        )
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

    /// check if verbose logging is enabled
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// check if this progress tracks items (vs bytes)
    pub fn is_items(&self) -> bool {
        self.kind == ProgressKind::Items
    }
}

pub struct ProgressReader<'a, R> {
    inner: R,
    progress: Option<&'a Progress>,
    bytes_read: u64,
}

impl<'a, R> ProgressReader<'a, R> {
    pub fn new(inner: R, progress: Option<&'a Progress>) -> Self {
        Self {
            inner,
            progress,
            bytes_read: 0,
        }
    }
}

impl<'a, R: Read> Read for ProgressReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read > 0 {
            self.bytes_read += read as u64;
            if let Some(progress) = self.progress {
                progress.update(self.bytes_read);
            }
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_disabled() {
        let progress = Progress::new(false, 1000, false);

        // Should not panic when progress is disabled
        progress.update(500);
        progress.finish_with_message("test");
        progress.finish();

        // Progress bar should be None when disabled
        assert!(progress.bar.is_none());
    }

    #[test]
    fn test_progress_enabled() {
        let progress = Progress::new(true, 1000, false);

        // Progress bar should exist when enabled
        assert!(progress.bar.is_some());

        // Should not panic with progress operations
        progress.update(500);
        progress.update(750);
        progress.finish_with_message("completed");
    }

    #[test]
    fn test_progress_zero_total() {
        let progress = Progress::new(true, 0, false);

        // Should handle zero total bytes without panic
        progress.update(0);
        progress.finish();
    }

    #[test]
    fn test_progress_update_beyond_total() {
        let progress = Progress::new(true, 100, false);

        // Should handle updates beyond total without panic
        progress.update(150);
        progress.finish();
    }

    #[test]
    fn test_progress_is_verbose() {
        // Enabled progress should be verbose
        let progress_enabled = Progress::new(true, 1000, true);
        assert!(progress_enabled.is_verbose());

        // Disabled progress should not be verbose
        let progress_disabled = Progress::new(false, 1000, false);
        assert!(!progress_disabled.is_verbose());
    }

    #[test]
    fn test_progress_verbose_without_bar() {
        let progress = Progress::new(false, 1000, true);
        assert!(progress.is_verbose());
        assert!(progress.bar.is_none());
    }
}
