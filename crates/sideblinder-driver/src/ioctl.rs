#![expect(unsafe_code, reason = "WDF/HID IOCTL handling requires unsafe FFI bindings")]

//! IOCTL dispatch for HID minidriver requests.
//!
//! HIDCLASS sends internal device control requests (IOCTLs) to the minidriver
//! for operations such as retrieving the HID descriptor, reading input reports,
//! and writing output reports (force-feedback commands).
//!
//! Additionally, the userspace app communicates with the driver through two
//! custom DeviceIoControl codes:
//!
//! | Code                          | Direction     | Purpose                      |
//! |-------------------------------|---------------|------------------------------|
//! | `IOCTL_SIDEBLINDER_UPDATE_INPUT` | App → Driver | Push new joystick state       |
//! | `IOCTL_SIDEBLINDER_GET_FFB`      | App ← Driver | Drain one FFB output report   |

use wdk_sys::*;

use crate::hid_descriptor::{HidClassDescriptor, REPORT_DESCRIPTOR, REPORT_DESCRIPTOR_LEN};
use crate::input_report::{InputSnapshot, REPORT_LEN};

// HID device attributes structure sent to HIDCLASS
#[repr(C)]
struct HID_DEVICE_ATTRIBUTES {
    Size: ULONG,
    VendorID: u16,
    ProductID: u16,
    VersionNumber: u16,
}

// ── Custom IOCTL codes ────────────────────────────────────────────────────────
//
// CTL_CODE(DeviceType, Function, Method, Access)
//   = (DeviceType << 16) | (Access << 14) | (Function << 2) | Method
//
// FILE_DEVICE_UNKNOWN = 0x22, METHOD_BUFFERED = 0, FILE_WRITE_DATA = 2,
// FILE_READ_DATA = 1

/// App → Driver: push a new [`InputSnapshot`] (buffered, write access).
pub const IOCTL_SIDEBLINDER_UPDATE_INPUT: u32 =
    (0x0022u32 << 16) | (0x0002u32 << 14) | (0x0800u32 << 2);

/// App ← Driver: pop the next FFB output report (buffered, read access).
pub const IOCTL_SIDEBLINDER_GET_FFB: u32 =
    (0x0022u32 << 16) | (0x0001u32 << 14) | (0x0801u32 << 2);

// ── HID IOCTL codes ───────────────────────────────────────────────────────────
// Standard HID IOCTL codes from hidclass.h
//
// CTL_CODE(DeviceType, Function, Method, Access) for HID IOCTL_HID_*
// DeviceType = 0x0B (FILE_DEVICE_KEYBOARD), Method = 0, Access = 0
// Each function increments by 4 (method bits are 00 = buffered)

const IOCTL_HID_GET_DEVICE_DESCRIPTOR: ULONG =
    (0x0B << 16) | (0x00 << 14) | (0x00 << 2) | 0; // Function 0x00
const IOCTL_HID_GET_REPORT_DESCRIPTOR: ULONG =
    (0x0B << 16) | (0x00 << 14) | (0x01 << 2) | 0; // Function 0x01
const IOCTL_HID_GET_DEVICE_ATTRIBUTES: ULONG =
    (0x0B << 16) | (0x00 << 14) | (0x02 << 2) | 0; // Function 0x02
const IOCTL_HID_READ_REPORT: ULONG =
    (0x0B << 16) | (0x00 << 14) | (0x03 << 2) | 0; // Function 0x03
const IOCTL_HID_WRITE_REPORT: ULONG =
    (0x0B << 16) | (0x00 << 14) | (0x04 << 2) | 0; // Function 0x04
const IOCTL_HID_GET_FEATURE: ULONG =
    (0x0B << 16) | (0x00 << 14) | (0x05 << 2) | 0; // Function 0x05
const IOCTL_HID_SET_FEATURE: ULONG =
    (0x0B << 16) | (0x00 << 14) | (0x06 << 2) | 0; // Function 0x06

// ── HID device attributes ─────────────────────────────────────────────────────

/// VID / PID / version reported to HIDCLASS via `IOCTL_HID_GET_DEVICE_ATTRIBUTES`.
const VID: u16 = 0x045E; // Microsoft
const PID: u16 = 0x001B; // Sidewinder FF2
const VERSION: u16 = 0x0100;

// ── IOCTL dispatcher ──────────────────────────────────────────────────────────

/// Handle an internal IOCTL forwarded by HIDCLASS, plus our custom codes.
pub unsafe extern "C" fn evt_io_internal_device_control(
    _queue: WDFQUEUE,
    request: WDFREQUEST,
    output_buffer_length: usize,
    input_buffer_length: usize,
    io_control_code: ULONG,
) {
    let status = match io_control_code {
        IOCTL_HID_GET_DEVICE_DESCRIPTOR => {
            handle_get_device_descriptor(request, output_buffer_length)
        }
        IOCTL_HID_GET_REPORT_DESCRIPTOR => {
            handle_get_report_descriptor(request, output_buffer_length)
        }
        IOCTL_HID_GET_DEVICE_ATTRIBUTES => handle_get_device_attributes(request, output_buffer_length),
        IOCTL_HID_READ_REPORT => {
            // Park the request — completed when the app pushes a new snapshot.
            // For now complete immediately with a zeroed (centred) report.
            handle_read_report(request, output_buffer_length)
        }
        IOCTL_HID_WRITE_REPORT => {
            handle_write_report(request, input_buffer_length)
        }
        IOCTL_HID_GET_FEATURE | IOCTL_HID_SET_FEATURE => {
            // Feature reports used for PID pool allocation; stub for now.
            STATUS_NOT_SUPPORTED
        }
        IOCTL_SIDEBLINDER_UPDATE_INPUT => {
            handle_update_input(request, input_buffer_length)
        }
        IOCTL_SIDEBLINDER_GET_FFB => {
            handle_get_ffb(request, output_buffer_length)
        }
        _ => STATUS_NOT_SUPPORTED,
    };

    // SAFETY: WdfRequestComplete must only be called once per request and only from
    // the callback that received it. We're in the dispatcher that was handed this request
    // by UMDF, and we complete it exactly once before returning.
    let completion_status = call_unsafe_wdf_function_binding!(WdfRequestComplete, request, status);
    // WdfRequestComplete can fail if the request is invalid or already completed,
    // but there's no way to propagate the error from this callback. In a production
    // driver, this would be logged to WMI or event tracing.
    let _ = completion_status;
}

// ── Individual handlers ───────────────────────────────────────────────────────

unsafe fn handle_get_device_descriptor(request: WDFREQUEST, out_len: usize) -> NTSTATUS {
    let desc = HidClassDescriptor::new();
    let needed = core::mem::size_of::<HidClassDescriptor>();
    if out_len < needed {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let mut buf_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut actual: usize = 0;
    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveOutputBuffer,
        request,
        needed,
        &mut buf_ptr,
        &mut actual
    );
    if !NT_SUCCESS(status) {
        return status;
    }

    core::ptr::copy_nonoverlapping(
        &desc as *const HidClassDescriptor as *const u8,
        buf_ptr as *mut u8,
        needed,
    );

    let status = call_unsafe_wdf_function_binding!(
        WdfRequestSetInformation,
        request,
        needed as u64
    );

    if NT_SUCCESS(status) { STATUS_SUCCESS } else { status }
}

unsafe fn handle_get_report_descriptor(request: WDFREQUEST, out_len: usize) -> NTSTATUS {
    if out_len < REPORT_DESCRIPTOR_LEN {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let mut buf_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut actual: usize = 0;
    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveOutputBuffer,
        request,
        REPORT_DESCRIPTOR_LEN,
        &mut buf_ptr,
        &mut actual
    );
    if !NT_SUCCESS(status) {
        return status;
    }

    core::ptr::copy_nonoverlapping(
        REPORT_DESCRIPTOR.as_ptr(),
        buf_ptr as *mut u8,
        REPORT_DESCRIPTOR_LEN,
    );

    let status = call_unsafe_wdf_function_binding!(
        WdfRequestSetInformation,
        request,
        REPORT_DESCRIPTOR_LEN as u64
    );

    if NT_SUCCESS(status) { STATUS_SUCCESS } else { status }
}

unsafe fn handle_get_device_attributes(request: WDFREQUEST, out_len: usize) -> NTSTATUS {
    let needed = core::mem::size_of::<HID_DEVICE_ATTRIBUTES>();
    if out_len < needed {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let mut buf_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut actual: usize = 0;
    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveOutputBuffer,
        request,
        needed,
        &mut buf_ptr,
        &mut actual
    );
    if !NT_SUCCESS(status) {
        return status;
    }

    let attrs = buf_ptr as *mut HID_DEVICE_ATTRIBUTES;
    (*attrs).Size = needed as ULONG;
    (*attrs).VendorID = VID;
    (*attrs).ProductID = PID;
    (*attrs).VersionNumber = VERSION;

    let status = call_unsafe_wdf_function_binding!(
        WdfRequestSetInformation,
        request,
        needed as u64
    );

    if NT_SUCCESS(status) { STATUS_SUCCESS } else { status }
}

unsafe fn handle_read_report(request: WDFREQUEST, out_len: usize) -> NTSTATUS {
    if out_len < REPORT_LEN {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let mut buf_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut actual: usize = 0;
    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveOutputBuffer,
        request,
        REPORT_LEN,
        &mut buf_ptr,
        &mut actual
    );
    if !NT_SUCCESS(status) {
        return status;
    }

    // Centred report (axes = 0, no buttons, hat = null)
    let report = InputSnapshot::default().to_report();
    core::ptr::copy_nonoverlapping(report.as_ptr(), buf_ptr as *mut u8, REPORT_LEN);

    let status = call_unsafe_wdf_function_binding!(
        WdfRequestSetInformation,
        request,
        REPORT_LEN as u64
    );

    if NT_SUCCESS(status) { STATUS_SUCCESS } else { status }
}

unsafe fn handle_write_report(request: WDFREQUEST, in_len: usize) -> NTSTATUS {
    if in_len == 0 {
        return STATUS_INVALID_PARAMETER;
    }

    let mut buf_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut actual: usize = 0;
    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveInputBuffer,
        request,
        1usize,
        &mut buf_ptr,
        &mut actual
    );
    if !NT_SUCCESS(status) {
        return status;
    }

    // The report is a raw HID PID output report destined for the FFB hardware.
    // A production driver would forward this to FfbQueue here; the app reads it
    // via IOCTL_SIDEBLINDER_GET_FFB.  For now we acknowledge receipt.
    let _raw = core::slice::from_raw_parts(buf_ptr as *const u8, actual);

    STATUS_SUCCESS
}

/// App → Driver: accept a new [`InputSnapshot`] pushed by the userspace app.
unsafe fn handle_update_input(request: WDFREQUEST, in_len: usize) -> NTSTATUS {
    let needed = core::mem::size_of::<InputSnapshot>();
    if in_len < needed {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let mut buf_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut actual: usize = 0;
    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveInputBuffer,
        request,
        needed,
        &mut buf_ptr,
        &mut actual
    );
    if !NT_SUCCESS(status) {
        return status;
    }

    // The snapshot is now available in the driver device context.  A production
    // driver would store it and complete any parked IOCTL_HID_READ_REPORT here.
    let _snap = &*(buf_ptr as *const InputSnapshot);

    STATUS_SUCCESS
}

/// App ← Driver: hand the app the next buffered FFB output report.
unsafe fn handle_get_ffb(_request: WDFREQUEST, out_len: usize) -> NTSTATUS {
    use crate::ffb_handler::MAX_FFB_REPORT_BYTES;

    if out_len < MAX_FFB_REPORT_BYTES {
        return STATUS_BUFFER_TOO_SMALL;
    }

    // A production driver would pop from FfbQueue here.  If empty, it would
    // park the request (STATUS_PENDING) and complete it on the next write.
    // For the skeleton we return STATUS_NO_MORE_ENTRIES to signal "nothing yet".
    STATUS_NO_MORE_ENTRIES
}
