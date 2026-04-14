//! HID device enumeration and discovery.
//!
//! On Windows, uses the `SetupDi*` family of APIs to walk the HID device
//! interface class and return the device paths of all matching joysticks.
//!
//! On non-Windows hosts the public surface is still present (returning an
//! empty iterator) so that the crate compiles in a cross-build / test
//! environment.

// ── Types ─────────────────────────────────────────────────────────────────────

/// VID and PID of the Microsoft Sidewinder Force Feedback 2.
pub const SIDEWINDER_FF2_VID: u16 = 0x045E;
pub const SIDEWINDER_FF2_PID: u16 = 0x001B;

/// A discovered HID device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidDeviceInfo {
    /// The `\\?\HID#...` path that can be passed to `CreateFileW`.
    pub path: String,
    /// USB Vendor ID.
    pub vendor_id: u16,
    /// USB Product ID.
    pub product_id: u16,
}

impl HidDeviceInfo {
    /// Returns `true` if this device is a Sidewinder Force Feedback 2.
    #[must_use]
    pub fn is_sidewinder_ff2(&self) -> bool {
        self.vendor_id == SIDEWINDER_FF2_VID && self.product_id == SIDEWINDER_FF2_PID
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Return information about all HID devices currently attached to the system.
///
/// # Errors
///
/// Returns a descriptive string on failure (`SetupDi` API error on Windows).
pub fn enumerate_hid_devices() -> Result<Vec<HidDeviceInfo>, String> {
    platform::enumerate_hid_devices_impl()
}

/// Return the first Sidewinder Force Feedback 2 found, or `None`.
///
/// # Errors
///
/// Returns a descriptive string if enumeration itself fails.
pub fn find_sidewinder() -> Result<Option<HidDeviceInfo>, String> {
    Ok(enumerate_hid_devices()?
        .into_iter()
        .find(HidDeviceInfo::is_sidewinder_ff2))
}

// ── Platform impls ────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "Windows HID enumeration requires raw Win32 SetupDi/HidD FFI calls"
)]
mod platform {
    use super::HidDeviceInfo;
    use std::mem;
    use windows_sys::Win32::{
        Devices::{
            DeviceAndDriverInstallation::{
                DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
                SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList,
                SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
                SetupDiGetDeviceInterfaceDetailW,
            },
            HumanInterfaceDevice::{HIDD_ATTRIBUTES, HidD_GetAttributes, HidD_GetHidGuid},
        },
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    };

    pub fn enumerate_hid_devices_impl() -> Result<Vec<HidDeviceInfo>, String> {
        let mut devices = Vec::new();

        // SAFETY: HidD_GetHidGuid writes to the GUID we pass; always succeeds.
        let hid_guid = unsafe {
            let mut guid = mem::zeroed();
            HidD_GetHidGuid(&raw mut guid);
            guid
        };

        // SAFETY: flags and GUID are well-formed; returns INVALID_HANDLE_VALUE
        // on error.
        let dev_info = unsafe {
            SetupDiGetClassDevsW(
                &raw const hid_guid,
                std::ptr::null(),
                std::ptr::null_mut(), // hwndParent: HWND = *mut c_void
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            )
        };

        // HDEVINFO is isize; failure is signalled by -1 (INVALID_HANDLE_VALUE).
        if dev_info == -1 {
            return Err("SetupDiGetClassDevsW failed".to_string());
        }

        let mut index: u32 = 0;
        loop {
            // SAFETY: SP_DEVICE_INTERFACE_DATA is a POD struct; zeroed is a valid starting state
            // before cbSize is set below.
            let mut iface_data: SP_DEVICE_INTERFACE_DATA = unsafe { mem::zeroed() };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "struct size always fits in u32"
            )]
            {
                iface_data.cbSize = mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            }

            // SAFETY: dev_info is valid; iface_data is correctly sized.
            let ok = unsafe {
                SetupDiEnumDeviceInterfaces(
                    dev_info,
                    std::ptr::null_mut(),
                    &raw const hid_guid,
                    index,
                    &raw mut iface_data,
                )
            };

            if ok == 0 {
                break; // ERROR_NO_MORE_ITEMS
            }

            // First call: get required buffer size.
            let mut required_size: u32 = 0;
            // SAFETY: passing null detail pointer and 0 size is the documented way to query the
            // required buffer size; dev_info and iface_data are valid.
            unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    dev_info,
                    &raw const iface_data,
                    std::ptr::null_mut(),
                    0,
                    &raw mut required_size,
                    std::ptr::null_mut(),
                )
            };

            if required_size == 0 {
                index += 1;
                continue;
            }

            // Allocate a properly-aligned buffer large enough for the detail struct.
            // required_size is in bytes; round up to whole struct units.
            let struct_size = mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            let n_structs = required_size as usize / struct_size + 1;
            // SAFETY: SP_DEVICE_INTERFACE_DETAIL_DATA_W is POD; zeroed is valid.
            let mut detail_buf: Vec<SP_DEVICE_INTERFACE_DETAIL_DATA_W> =
                unsafe { vec![mem::zeroed(); n_structs] };

            // Write cbSize into the first DWORD of the struct.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "struct size always fits in u32"
            )]
            {
                detail_buf[0].cbSize = struct_size as u32;
            }

            let detail_ptr = detail_buf.as_mut_ptr();

            // SAFETY: detail_buf is large enough (required_size bytes).
            let ok = unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    dev_info,
                    &raw const iface_data,
                    detail_ptr,
                    required_size,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };

            if ok != 0 {
                // DevicePath is a WCHAR array starting at offset 4 in the struct.
                // SAFETY: ok != 0 means SetupDiGetDeviceInterfaceDetailW succeeded and wrote a
                // valid SP_DEVICE_INTERFACE_DETAIL_DATA_W into detail_buf.
                let path_ptr = unsafe { (*detail_ptr).DevicePath.as_ptr() };
                let path = wide_ptr_to_string(path_ptr);

                // Open the device to query VID/PID via HidD_GetAttributes.
                if let Some(info) = try_get_device_info(&path) {
                    devices.push(info);
                }
            }

            index += 1;
        }

        // SAFETY: dev_info is the valid handle returned above.
        unsafe { SetupDiDestroyDeviceInfoList(dev_info) };

        Ok(devices)
    }

    /// Open the device at `path` and query its VID/PID.  Returns `None` if the
    /// device can't be opened (e.g. access denied, already in exclusive use).
    fn try_get_device_info(path: &str) -> Option<HidDeviceInfo> {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

        // SAFETY: wide is null-terminated; constants are well-formed.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0, // 0 = query-only open
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(), // hTemplateFile: HANDLE = *mut c_void
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return None;
        }

        // SAFETY: HIDD_ATTRIBUTES is a POD struct; zeroed is valid before Size is set below.
        let mut attrs: HIDD_ATTRIBUTES = unsafe { mem::zeroed() };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "struct size always fits in u32"
        )]
        {
            attrs.Size = mem::size_of::<HIDD_ATTRIBUTES>() as u32;
        }

        // SAFETY: handle is valid; attrs is correctly sized.
        let ok = unsafe { HidD_GetAttributes(handle, &raw mut attrs) };

        // SAFETY: handle is valid and owned here.
        unsafe { CloseHandle(handle) };

        if ok == 0 {
            return None;
        }

        Some(HidDeviceInfo {
            path: path.to_string(),
            vendor_id: attrs.VendorID,
            product_id: attrs.ProductID,
        })
    }

    /// Convert a `*const u16` null-terminated wide string to a `String`.
    fn wide_ptr_to_string(ptr: *const u16) -> String {
        let mut len = 0;
        // SAFETY: caller guarantees ptr points to a null-terminated WCHAR array.
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: ptr is valid for `len` elements (loop above counted up to the null terminator).
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        String::from_utf16_lossy(slice)
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::HidDeviceInfo;

    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the Windows impl which can return Err"
    )]
    pub fn enumerate_hid_devices_impl() -> Result<Vec<HidDeviceInfo>, String> {
        // No HID enumeration outside of Windows; return empty list so callers
        // can test the non-finding path without platform APIs.
        Ok(Vec::new())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerate_returns_ok_on_non_windows() {
        // On Linux/macOS (CI / dev machine) we just want an empty, not an error.
        let result = enumerate_hid_devices();
        assert!(result.is_ok());
    }

    #[test]
    fn find_sidewinder_none_on_non_windows() {
        let result = find_sidewinder();
        assert!(result.is_ok());
        // On a non-Windows dev machine there's no Sidewinder attached.
        #[cfg(not(target_os = "windows"))]
        assert!(
            result
                .expect("find_sidewinder should succeed on non-Windows")
                .is_none()
        );
    }

    #[test]
    fn is_sidewinder_ff2_positive() {
        let info = HidDeviceInfo {
            path: "\\\\?\\HID#VID_045E&PID_001B".to_string(),
            vendor_id: 0x045E,
            product_id: 0x001B,
        };
        assert!(info.is_sidewinder_ff2());
    }

    #[test]
    fn is_sidewinder_ff2_negative_wrong_pid() {
        let info = HidDeviceInfo {
            path: "\\\\?\\HID#VID_045E&PID_0001".to_string(),
            vendor_id: 0x045E,
            product_id: 0x0001,
        };
        assert!(!info.is_sidewinder_ff2());
    }
}
