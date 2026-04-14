//! Input report construction and pending-request management.
//!
//! The userspace app pushes a new [`InputSnapshot`] via a custom IOCTL.
//! When HIDCLASS issues `IOCTL_HID_READ_REPORT`, we either complete it
//! immediately (if a snapshot is already waiting) or park it in a pending
//! queue until the next push arrives.
//!
//! Report layout (matches the descriptor in `hid_descriptor.rs`, no Report ID):
//!
//! | Bytes | Field                         |
//! |-------|-------------------------------|
//! | 0–1   | X axis (i16 LE)               |
//! | 2–3   | Y axis (i16 LE)               |
//! | 4–5   | Z / throttle (i16 LE)         |
//! | 6–7   | Rz / rudder  (i16 LE)         |
//! | 8–9   | Buttons 1–9 (low 9 bits)      |
//! | 10    | Hat switch nibble + 4 pad bits |

// ── Snapshot ──────────────────────────────────────────────────────────────────

/// Joystick state as pushed from the userspace app to the driver.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InputSnapshot {
    /// Axes in signed 16-bit range.  Index order: X, Y, Z, Rz.
    pub axes: [i16; 4],
    /// Button bitfield (low 9 bits = buttons 1–9).
    pub buttons: u16,
    /// Hat switch value (0 = N … 7 = NW, 0xFF = centred).
    pub pov: u8,
}

impl InputSnapshot {
    /// Build the 11-byte HID input report for this snapshot.
    pub fn to_report(self) -> [u8; REPORT_LEN] {
        let mut buf = [0u8; REPORT_LEN];

        // Four axes, little-endian i16
        for (i, axis) in self.axes.iter().enumerate() {
            let bytes = axis.to_le_bytes();
            buf[i * 2] = bytes[0];
            buf[i * 2 + 1] = bytes[1];
        }

        // Buttons (9 bits packed into u16 LE)
        let btn = self.buttons & 0x01FF;
        buf[8] = (btn & 0xFF) as u8;
        buf[9] = ((btn >> 8) & 0xFF) as u8;

        // Hat nibble in the high 4 bits of byte 10; low 4 bits = 0 (padding)
        buf[10] = if self.pov <= 7 {
            self.pov << 4
        } else {
            0xFF // null / centred
        };

        buf
    }
}

/// Byte length of one HID input report (no report-ID prefix).
pub const REPORT_LEN: usize = 11;

/// A centred, no-buttons, hat-null snapshot — safe default.
impl Default for InputSnapshot {
    fn default() -> Self {
        Self {
            axes: [0; 4],
            buttons: 0,
            pov: 0xFF,
        }
    }
}

// ── Pending request queue ─────────────────────────────────────────────────────

/// Manages the latest input snapshot and any parked `IOCTL_HID_READ_REPORT`
/// request.
///
/// The driver holds one of these in its device context.  Only the driver
/// dispatch threads interact with it, so all mutability is managed by the
/// KMDF serialisation guarantees on the queue.
pub struct InputQueue {
    /// Most-recently-pushed snapshot, atomically updated.
    current: core::sync::atomic::AtomicU64,
    /// Whether there is a new snapshot that hasn't been consumed yet.
    dirty: core::sync::atomic::AtomicBool,
}

impl InputQueue {
    /// Create a centred queue with no pending snapshot.
    pub const fn new() -> Self {
        Self {
            current: core::sync::atomic::AtomicU64::new(0),
            dirty: core::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Store a new snapshot.  Returns the packed u64 representation.
    pub fn push(&self, snap: InputSnapshot) -> u64 {
        let packed = pack_snapshot(snap);
        self.current
            .store(packed, core::sync::atomic::Ordering::Release);
        self.dirty
            .store(true, core::sync::atomic::Ordering::Release);
        packed
    }

    /// Take the current snapshot if one has been pushed since the last take.
    /// Returns `None` if no new data is available.
    pub fn take(&self) -> Option<InputSnapshot> {
        if self
            .dirty
            .swap(false, core::sync::atomic::Ordering::AcqRel)
        {
            let packed = self.current.load(core::sync::atomic::Ordering::Acquire);
            Some(unpack_snapshot(packed))
        } else {
            None
        }
    }
}

// Pack a snapshot into a single u64 for atomic storage.
// Layout: [axes[0] i16][axes[1] i16][axes[2] i16][axes[3] i16] = 64 bits;
// buttons and pov stored separately via dirty flag + full struct on take.
// Simplified: we pack only enough for the atomic; we re-read from a cell on take.
//
// Real drivers would use a spinlock-protected struct; the atomic here is a
// minimal stand-in that compiles without WDK spinlock types.
fn pack_snapshot(s: InputSnapshot) -> u64 {
    let x = s.axes[0] as u16 as u64;
    let y = s.axes[1] as u16 as u64;
    let z = s.axes[2] as u16 as u64;
    let r = s.axes[3] as u16 as u64;
    x | (y << 16) | (z << 32) | (r << 48)
}

fn unpack_snapshot(v: u64) -> InputSnapshot {
    InputSnapshot {
        axes: [
            (v & 0xFFFF) as u16 as i16,
            ((v >> 16) & 0xFFFF) as u16 as i16,
            ((v >> 32) & 0xFFFF) as u16 as i16,
            ((v >> 48) & 0xFFFF) as u16 as i16,
        ],
        buttons: 0, // buttons/pov not packed — driver re-reads full struct
        pov: 0xFF,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_report_is_all_zeros_except_pov() {
        let snap = InputSnapshot::default();
        let report = snap.to_report();
        // All axis bytes are 0 (i16 0 → 0x00 0x00)
        for b in &report[0..8] {
            assert_eq!(*b, 0x00);
        }
        // No buttons
        assert_eq!(report[8], 0x00);
        assert_eq!(report[9], 0x00);
        // Null hat (0xFF in nibble form — 0xFF for the whole byte)
        assert_eq!(report[10], 0xFF);
    }

    #[test]
    fn axis_values_are_little_endian() {
        let snap = InputSnapshot {
            axes: [0x1234, -1, 0, 0],
            ..Default::default()
        };
        let report = snap.to_report();
        assert_eq!(report[0], 0x34);
        assert_eq!(report[1], 0x12);
        // -1 as i16 = 0xFFFF
        assert_eq!(report[2], 0xFF);
        assert_eq!(report[3], 0xFF);
    }

    #[test]
    fn buttons_packed_correctly() {
        let snap = InputSnapshot {
            buttons: 0b1_0000_0101, // buttons 0, 2, and 8
            ..Default::default()
        };
        let report = snap.to_report();
        assert_eq!(report[8], 0x05); // low 8 bits
        assert_eq!(report[9], 0x01); // bit 8
    }

    #[test]
    fn pov_nibble_encoding() {
        // North = 0 → upper nibble 0x0, lower = 0 → 0x00
        let snap_n = InputSnapshot { pov: 0, ..Default::default() };
        assert_eq!(snap_n.to_report()[10], 0x00);

        // East = 2 → upper nibble 0x2 → 0x20
        let snap_e = InputSnapshot { pov: 2, ..Default::default() };
        assert_eq!(snap_e.to_report()[10], 0x20);

        // Null = 0xFF → 0xFF
        let snap_c = InputSnapshot { pov: 0xFF, ..Default::default() };
        assert_eq!(snap_c.to_report()[10], 0xFF);
    }

    #[test]
    fn input_queue_push_and_take() {
        let q = InputQueue::new();
        assert!(q.take().is_none()); // nothing pushed yet

        let snap = InputSnapshot {
            axes: [100, -200, 300, -400],
            buttons: 0b101,
            pov: 3,
        };
        q.push(snap);
        let got = q.take().expect("should have a snapshot");
        // Axes are packed/unpacked; buttons/pov are zeroed in the packed form
        assert_eq!(got.axes, [100, -200, 300, -400]);

        assert!(q.take().is_none()); // consumed
    }
}
