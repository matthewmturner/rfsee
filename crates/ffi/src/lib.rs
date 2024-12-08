use std::collections::HashMap;
use std::ffi::*;
use std::os::raw::c_char;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[repr(C)]
pub struct SaveResult {
    error: bool,
}

// Exposed via an opaque pointer via FFI
type TermFrequencies = HashMap<CString, f64>;

#[no_mangle]
pub extern "C" fn create_term_freqs() -> *mut TermFrequencies {
    let map: HashMap<CString, f64> = HashMap::new();
    let boxed = Box::new(map);
    Box::into_raw(boxed)
}

/// ChatGPT created the safety docs
/// Inserts a key-value pair into the `TermFrequencies` map.
///
/// # Safety
/// - `term_freqs` must be a valid, non-null pointer to a `TermFrequencies` instance created by Rust.
/// - The caller must ensure that `term_freqs` is not being accessed concurrently or mutably elsewhere during this call.
/// - `key` must be a valid, non-null pointer to a null-terminated C string. The string must remain valid
///   for the duration of this function call.
/// - The function will return immediately if `term_freqs` or `key` is null.
/// - Undefined behavior may occur if the requirements above are not met.
#[no_mangle]
pub unsafe extern "C" fn insert_term_freqs(
    term_freqs: *mut TermFrequencies,
    key: *const c_char,
    value: f64,
) {
    if term_freqs.is_null() || key.is_null() {
        return;
    }
    let term_freqs = unsafe { &mut *term_freqs };
    let key = unsafe { CStr::from_ptr(key) };
    match key.to_str() {
        Ok(_) => {
            term_freqs.insert(key.to_owned(), value);
        }
        Err(_) => {
            eprintln!("ERROR: Unable to convert key to UTF-8")
        }
    }
}

/// ChatGPT created the safety docs
/// Retrieves a value from the `TermFrequencies` map by key.
///
/// # Safety
/// - `term_freqs` must be a valid, non-null pointer to a `TermFrequencies` instance created by Rust.
/// - The caller must ensure that `term_freqs` is not being accessed concurrently or mutably elsewhere during this call.
/// - `key` must be a valid, non-null pointer to a null-terminated C string. The string must remain valid
///   for the duration of this function call.
/// - The function will return a null pointer if:
///   - `term_freqs` or `key` is null.
///   - The key does not exist in the map.
/// - The returned pointer is valid only as long as `term_freqs` remains valid and is not modified.
/// - Undefined behavior may occur if the requirements above are not met.
#[no_mangle]
pub unsafe extern "C" fn get_term_freqs(
    term_freqs: *const TermFrequencies,
    key: *const c_char,
) -> *const f64 {
    if term_freqs.is_null() || key.is_null() {
        // TODO: Maybe return an int that represents error types instead?
        return std::ptr::null();
    }

    let term_freqs = unsafe { &*term_freqs };
    let key = unsafe { CStr::from_ptr(key) };

    // Make sure the key is UTF-8
    match key.to_str() {
        Ok(_) => match term_freqs.get(key) {
            Some(v) => v,
            None => std::ptr::null(),
        },
        Err(_) => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn hello_world() -> i32 {
    42
}

#[no_mangle]
pub extern "C" fn save_json() -> SaveResult {
    match api::save_json() {
        Ok(_) => SaveResult { error: false },
        Err(_) => SaveResult { error: true },
    }
}

#[no_mangle]
pub extern "C" fn save_input_number_as_json(val: i32) -> SaveResult {
    match api::save_input_number_as_json(val) {
        Ok(_) => SaveResult { error: false },
        Err(_) => SaveResult { error: true },
    }
}

/// Take input number and save it to provided path as JSON
/// # Safety
/// This function is marked as `unsafe` because it takes a raw pointer (`path`) as an argument.
/// The caller must ensure the following:
///
/// - `path` must be non-null. Passing a null pointer will result in undefined behavior.
/// - `path` must point to a valid null-terminated C-style string.
/// - The string referenced by `path` must remain valid for the duration of the function call.
/// - The caller must ensure proper synchronization if this function is called concurrently,
///   as it could interact with shared resources in `api::save_input_number_as_json_to_custom_path`.
///
/// Failure to adhere to these requirements may result in undefined behavior or program crashes.
#[no_mangle]
pub unsafe extern "C" fn save_input_number_as_json_to_custom_path(
    val: i32,
    path: *const c_char,
) -> SaveResult {
    if path.is_null() {
        return SaveResult { error: true };
    }

    let c_str = unsafe { CStr::from_ptr(path) };
    match c_str.to_str() {
        Ok(s) => match api::save_input_number_as_json_to_custom_path(val, s) {
            Ok(_) => SaveResult { error: false },
            Err(_) => SaveResult { error: true },
        },
        Err(_) => SaveResult { error: true },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
