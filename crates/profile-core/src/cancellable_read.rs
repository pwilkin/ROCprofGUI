use std::{
    io::{self, Read},
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("CSV import cancelled".into())
    } else {
        Ok(())
    }
}

pub(crate) struct CancellableRead<'a, R> {
    inner: R,
    cancelled: &'a AtomicBool,
    message: &'static str,
}
impl<'a, R> CancellableRead<'a, R> {
    pub(crate) fn new(inner: R, cancelled: &'a AtomicBool) -> Self {
        Self::with_message(inner, cancelled, "CSV import cancelled")
    }
    pub(crate) fn with_message(inner: R, cancelled: &'a AtomicBool, message: &'static str) -> Self {
        Self {
            inner,
            cancelled,
            message,
        }
    }
}
impl<R: Read> Read for CancellableRead<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other(self.message));
        }
        let count = bytes.len().min(64 * 1024);
        let read = self.inner.read(&mut bytes[..count])?;
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other(self.message));
        }
        Ok(read)
    }
}
