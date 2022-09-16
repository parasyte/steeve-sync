use parking_lot::Mutex;
use std::{collections::VecDeque, io::Write, ops::Deref, sync::Arc};

/// The logger holds log lines for [`MemLogger`].
///
/// It derefs to the inner mutex.
///
/// # Examples
///
/// ```
/// # use steeve_sync::logger::Logger;
/// let logger = Logger::default();
/// let mut guard = logger.lock();
/// guard.push_back("Hello, world!".to_string());
/// assert_eq!(guard.len(), 1);
/// ```
#[derive(Clone, Debug, Default)]
pub struct Logger(Arc<Mutex<VecDeque<String>>>);

/// An in-memory logger that removes old log lines with a configurable cap.
#[derive(Debug)]
pub struct MemLogger {
    max_lines: usize,
    buffer: Vec<u8>,
    logger: Logger,
}

impl Deref for Logger {
    type Target = Mutex<VecDeque<String>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl MemLogger {
    /// Create a new in-memory logger with the given `max_lines` cap.
    pub fn new(max_lines: usize, logger: &Logger) -> Self {
        Self {
            max_lines,
            buffer: Vec::new(),
            logger: logger.clone(),
        }
    }
}

impl Write for MemLogger {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.write_all(buf)?;

        // Flush when a new-line is written
        if self.buffer.iter().any(|b| char::from(*b) == '\n') {
            self.flush()?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Convert bytes into a string with lossy UTF-8 encoding
        let buffer = String::from_utf8_lossy(&self.buffer);

        let mut guard = self.logger.lock();

        // Write all lines
        for line in buffer.lines() {
            if line.is_empty() {
                continue;
            }

            if guard.len() >= self.max_lines {
                guard.pop_front();
            }
            guard.push_back(line.to_string());
        }

        // Consume the buffer
        self.buffer.clear();

        Ok(())
    }
}
