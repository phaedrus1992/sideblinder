//! Sidewinder Force Feedback 2 — UMDF2 HID minidriver
//!
//! This crate implements a Windows UMDF2 driver that acts as a virtual HID
//! device, translating Sidewinder FFB2 gameport protocol data into standard
//! HID reports and force-feedback commands.

/// Call a WDF function with automatic error handling.
///
/// This macro wraps unsafe WDF function bindings and returns the NTSTATUS result.
#[macro_export]
macro_rules! call_unsafe_wdf_function_binding {
    ($func:ident, $($arg:expr),*) => {{
        unsafe {
            wdk_sys::$func($($arg),*)
        }
    }};
}

mod ffb_handler;
mod hid_descriptor;
mod input_report;
mod ioctl;

use wdk_sys::*;

// WDF IO Queue configuration constants
const WDF_DEFAULT: i32 = 0; // WdfDefault tri-state value
const WDF_IO_QUEUE_DISPATCH_PARALLEL: i32 = 0; // WdfIoQueueDispatchParallel

/// Driver entry point called by the Windows kernel.
///
/// Creates a WDF driver object and registers the [`evt_driver_device_add`]
/// callback which is invoked each time the PnP manager enumerates our device.
#[unsafe(export_name = "DriverEntry")]
pub unsafe extern "system" fn driver_entry(
    driver_object: PDRIVER_OBJECT,
    registry_path: PUNICODE_STRING,
) -> NTSTATUS {
    let mut driver_handle: WDFDRIVER = core::ptr::null_mut();

    let mut driver_config = WDF_DRIVER_CONFIG {
        Size: core::mem::size_of::<WDF_DRIVER_CONFIG>() as ULONG,
        EvtDriverDeviceAdd: Some(evt_driver_device_add),
        EvtDriverUnload: None,
        DriverInitFlags: 0,
        DriverPoolTag: 0,
    };

    let status = call_unsafe_wdf_function_binding!(
        WdfDriverCreate,
        driver_object,
        registry_path,
        WDF_NO_OBJECT_ATTRIBUTES,
        &mut driver_config,
        &mut driver_handle
    );

    status
}

/// PnP callback: configure the device as a filter driver and create the I/O
/// queue that will handle HID IOCTLs forwarded by HIDCLASS.
unsafe extern "C" fn evt_driver_device_add(
    _driver: WDFDRIVER,
    mut device_init: PWDFDEVICE_INIT,
) -> NTSTATUS {
    // Mark ourselves as a filter driver in the HID stack.
    call_unsafe_wdf_function_binding!(WdfFdoInitSetFilter, device_init);

    // Create the device object.
    let mut device: WDFDEVICE = core::ptr::null_mut();
    let status = call_unsafe_wdf_function_binding!(
        WdfDeviceCreate,
        &mut device_init,
        WDF_NO_OBJECT_ATTRIBUTES,
        &mut device
    );

    if !NT_SUCCESS(status) {
        return status;
    }

    // Create a parallel default queue for internal device control requests
    // (HID IOCTLs arrive as internal IOCTLs from HIDCLASS).
    let mut queue_config = WDF_IO_QUEUE_CONFIG {
        Size: core::mem::size_of::<WDF_IO_QUEUE_CONFIG>() as ULONG,
        PowerManaged: WDF_DEFAULT,
        DefaultQueue: BOOLEAN::from(true),
        DispatchType: WDF_IO_QUEUE_DISPATCH_PARALLEL,
        EvtIoInternalDeviceControl: Some(ioctl::evt_io_internal_device_control),
        // Unused callbacks — set to None.
        EvtIoDefault: None,
        EvtIoRead: None,
        EvtIoWrite: None,
        EvtIoDeviceControl: None,
        EvtIoStop: None,
        EvtIoResume: None,
        EvtIoCanceledOnQueue: None,
        Settings: unsafe { core::mem::zeroed() },
        Driver: core::ptr::null_mut(),
    };

    let mut queue: WDFQUEUE = core::ptr::null_mut();
    let status = call_unsafe_wdf_function_binding!(
        WdfIoQueueCreate,
        device,
        &mut queue_config,
        WDF_NO_OBJECT_ATTRIBUTES,
        &mut queue
    );

    status
}

