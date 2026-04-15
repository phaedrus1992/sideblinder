//! Force-feedback output report handler.
//!
//! When HIDCLASS delivers `IOCTL_HID_WRITE_REPORT`, the raw HID PID output
//! report is buffered here.  The userspace app drains it by issuing
//! `IOCTL_SIDEBLINDER_GET_FFB` — the driver either completes the request
//! immediately with the buffered report, or parks it until the next write.

/// Maximum size of one buffered FFB report (Set Effect is the largest at
/// roughly 20 bytes with all fields; 64 bytes gives comfortable headroom).
pub const MAX_FFB_REPORT_BYTES: usize = 64;

/// A raw HID PID output or feature report captured from the game.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfbReport {
    /// Number of valid bytes in `data`.
    pub len: u8,
    /// Raw bytes (first byte is always the HID report ID).
    pub data: [u8; MAX_FFB_REPORT_BYTES],
}

impl FfbReport {
    /// Construct from a raw byte slice.  Truncates silently to
    /// `MAX_FFB_REPORT_BYTES`.
    pub fn from_bytes(src: &[u8]) -> Self {
        let mut report = Self {
            len: src.len().min(MAX_FFB_REPORT_BYTES) as u8,
            data: [0u8; MAX_FFB_REPORT_BYTES],
        };
        report.data[..report.len as usize].copy_from_slice(&src[..report.len as usize]);
        report
    }

    /// Return the valid portion of `data` as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }

    /// HID Report ID (the first byte), or `None` for an empty report.
    pub fn report_id(&self) -> Option<u8> {
        if self.len > 0 { Some(self.data[0]) } else { None }
    }
}

/// Ring buffer holding the most recent unread FFB reports.
///
/// Sized for up to `CAPACITY` reports; oldest entries are overwritten when
/// full (the physical hardware is the authoritative source of truth, so
/// dropping a stale intermediate state is acceptable).
pub struct FfbQueue {
    buf: [FfbReport; Self::CAPACITY],
    head: core::sync::atomic::AtomicUsize, // next write position
    tail: core::sync::atomic::AtomicUsize, // next read position
}

impl FfbQueue {
    const CAPACITY: usize = 16;

    /// Create an empty queue.
    pub const fn new() -> Self {
        Self {
            buf: [FfbReport {
                len: 0,
                data: [0u8; MAX_FFB_REPORT_BYTES],
            }; Self::CAPACITY],
            head: core::sync::atomic::AtomicUsize::new(0),
            tail: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Push a report.  Overwrites the oldest entry if full.
    pub fn push(&mut self, report: FfbReport) {
        let head = self.head.load(core::sync::atomic::Ordering::Acquire);
        self.buf[head % Self::CAPACITY] = report;
        let next = (head + 1) % Self::CAPACITY;
        self.head
            .store(next, core::sync::atomic::Ordering::Release);

        // If we just lapped the tail, advance it too.
        let tail = self.tail.load(core::sync::atomic::Ordering::Acquire);
        if next == tail {
            self.tail
                .store((tail + 1) % Self::CAPACITY, core::sync::atomic::Ordering::Release);
        }
    }

    /// Pop the oldest report, or `None` if the queue is empty.
    pub fn pop(&mut self) -> Option<FfbReport> {
        let head = self.head.load(core::sync::atomic::Ordering::Acquire);
        let tail = self.tail.load(core::sync::atomic::Ordering::Acquire);
        if head == tail {
            return None;
        }
        let report = self.buf[tail % Self::CAPACITY];
        self.tail
            .store((tail + 1) % Self::CAPACITY, core::sync::atomic::Ordering::Release);
        Some(report)
    }

    /// Returns `true` if the queue has at least one pending report.
    pub fn is_nonempty(&self) -> bool {
        self.head.load(core::sync::atomic::Ordering::Acquire)
            != self.tail.load(core::sync::atomic::Ordering::Acquire)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_and_round_trip() {
        let src = [0x05u8, 0x01, 0x00, 0xFF, 0x7F];
        let r = FfbReport::from_bytes(&src);
        assert_eq!(r.len, 5);
        assert_eq!(r.as_bytes(), &src);
        assert_eq!(r.report_id(), Some(0x05));
    }

    #[test]
    fn empty_report_has_no_id() {
        let r = FfbReport::from_bytes(&[]);
        assert_eq!(r.len, 0);
        assert_eq!(r.report_id(), None);
    }

    #[test]
    fn truncates_to_max() {
        let src = [0u8; MAX_FFB_REPORT_BYTES + 10];
        let r = FfbReport::from_bytes(&src);
        assert_eq!(r.len as usize, MAX_FFB_REPORT_BYTES);
    }

    #[test]
    fn queue_push_pop_fifo() {
        let mut q = FfbQueue::new();
        assert!(!q.is_nonempty());

        let r1 = FfbReport::from_bytes(&[0x01, 0xAA]);
        let r2 = FfbReport::from_bytes(&[0x0A, 0xBB]);
        q.push(r1);
        q.push(r2);

        assert!(q.is_nonempty());
        assert_eq!(q.pop().unwrap().as_bytes(), &[0x01, 0xAA]);
        assert_eq!(q.pop().unwrap().as_bytes(), &[0x0A, 0xBB]);
        assert!(q.pop().is_none());
    }
}
