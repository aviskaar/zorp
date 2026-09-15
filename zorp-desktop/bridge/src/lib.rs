mod env;
mod server;

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;

#[no_mangle]
pub extern "C" fn zorp_bridge_repair_path() -> i32 {
    if env::repair_path() {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn zorp_bridge_start_server(
    requested_port: u16,
    resource_dir: *const c_char,
    out_port: *mut u16,
) -> i32 {
    if out_port.is_null() {
        return -1;
    }

    let bundle_dir: Option<PathBuf> = if resource_dir.is_null() {
        None
    } else {
        unsafe {
            CStr::from_ptr(resource_dir)
                .to_str()
                .ok()
                .map(PathBuf::from)
        }
    };

    match server::start_background_server(requested_port, bundle_dir) {
        Ok(addr) => {
            unsafe {
                *out_port = addr.port();
            }
            0
        }
        Err(err) => {
            eprintln!("zorp_bridge_start_server error: {err}");
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn zorp_bridge_stop_server() {
    server::stop_server();
}
