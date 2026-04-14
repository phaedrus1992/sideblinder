//! HID transport and device abstraction for the Sidewinder Force Feedback 2 joystick.

// `expect()` is the idiomatic way to assert infallible conditions in tests.
// Suppress the expect_used warning within test compilation units.
#![cfg_attr(
    test,
    expect(clippy::expect_used, reason = "expect() is idiomatic in tests")
)]

pub mod device;
pub mod enumerate;
pub mod ffb;
pub mod hid_transport;
pub mod input;
