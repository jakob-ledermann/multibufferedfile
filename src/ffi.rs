use core::slice;
use std::cell::RefCell;
use std::io::{ErrorKind, Read, Write};
use std::os::raw::c_int;
use std::ptr;
use std::{ffi::CStr, os::raw::c_char, path::PathBuf};
use tracing::warn;

use crate::{BufferedFile, BufferedFileErrors, BufferedFileReader, BufferedFileWriter};

#[derive(Debug)]
pub enum Error {
    NonUtf8Path,
    InvalidPointer,
    BufferTooLong,
    BufferedFileErrors(BufferedFileErrors),
}

#[repr(i64)]
pub enum ErrorCode {
    Success = 0,
    NonUtf8Path = -200,
    BufferTooLong = -201,
    InvalidPointer = -202,
    FileNotFound = -1,
    UnknownIoError = -3,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<Error>>  = RefCell::new(None);
}

pub type FileReader = *mut BufferedFileReader<std::fs::File>;

pub type FileWriter = *mut BufferedFileWriter<std::fs::File>;

impl From<ErrorCode> for i64 {
    fn from(other: ErrorCode) -> Self {
        other as i64
    }
}

impl From<&std::io::Error> for ErrorCode {
    fn from(other: &std::io::Error) -> Self {
        match other.kind() {
            ErrorKind::NotFound => ErrorCode::FileNotFound,
            _ => ErrorCode::UnknownIoError,
        }
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_open_read(path: *const c_char) -> FileReader {
    let path = unsafe { CStr::from_ptr(path) };
    let path = match path.to_str() {
        Ok(path) => path,
        Err(_err) => {
            // TODO Error handling in ffi
            LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::NonUtf8Path));
            return ptr::null_mut();
        }
    };
    let path = PathBuf::from(path);

    let file = match BufferedFile::new(&path) {
        Ok(file) => file,
        Err(inner) => {
            // TODO Error handling in ffi
            LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::BufferedFileErrors(inner)));
            return ptr::null_mut();
        }
    };

    match file.read() {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let reference = std::boxed::Box::<_>::leak(boxed);
            reference as *mut _
        }
        Err(inner) => {
            LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::BufferedFileErrors(inner)));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_open_write(path: *const c_char) -> FileWriter {
    let path = unsafe { CStr::from_ptr(path) };
    let path = match path.to_str() {
        Ok(path) => path,
        Err(_err) => {
            // TODO Error handling in ffi
            LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::NonUtf8Path));
            return ptr::null_mut();
        }
    };
    let path = PathBuf::from(path);

    let file = match BufferedFile::new(&path) {
        Ok(file) => file,
        Err(inner) => {
            // TODO Error handling in ffi
            LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::BufferedFileErrors(inner)));
            return ptr::null_mut();
        }
    };

    match file.write() {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let reference = std::boxed::Box::<_>::leak(boxed);
            reference as *mut _
        }
        Err(inner) => {
            LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::BufferedFileErrors(inner)));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_read(reader: FileReader, buffer: *mut u8, buffer_len: usize) -> i64 {
    if buffer_len > usize::try_from(i64::MAX).unwrap_or(buffer_len) {
        LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::BufferTooLong));
        return ErrorCode::BufferTooLong.into();
    }

    if reader.is_null() {
        LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::InvalidPointer));
        return ErrorCode::InvalidPointer.into();
    }

    if buffer.is_null() {
        LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::InvalidPointer));
        return ErrorCode::InvalidPointer.into();
    }

    let reader = unsafe { &mut *reader };
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, buffer_len) };
    match reader.read(buf) {
        Ok(amt) => i64::try_from(amt).expect("We checked the buffer size should fit into i64"),
        Err(err) => {
            let error = ErrorCode::from(&err);
            LAST_ERROR.with(|x| {
                *x.borrow_mut() = Some(Error::BufferedFileErrors(BufferedFileErrors::IoError(err)))
            });
            error.into()
        }
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_write(
    writer: FileWriter,
    buffer: *mut u8,
    buffer_len: usize,
) -> i64 {
    if buffer_len > usize::try_from(i64::MAX).unwrap_or(buffer_len) {
        LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::BufferTooLong));
        return ErrorCode::BufferTooLong.into();
    }

    if writer.is_null() {
        LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::InvalidPointer));
        return ErrorCode::InvalidPointer.into();
    }

    if buffer.is_null() {
        LAST_ERROR.with(|x| *x.borrow_mut() = Some(Error::InvalidPointer));
        return ErrorCode::InvalidPointer.into();
    }

    let writer = unsafe { &mut *writer };
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, buffer_len) };
    match writer.write(buf) {
        Ok(amt) => i64::try_from(amt).expect("We checked the buffer size should fit into i64"),
        Err(err) => {
            let error = ErrorCode::from(&err);
            LAST_ERROR.with(|x| {
                *x.borrow_mut() = Some(Error::BufferedFileErrors(BufferedFileErrors::IoError(err)))
            });
            error.into()
        }
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_close_read(reader: FileReader) {
    if !reader.is_null() {
        let boxed = unsafe { Box::from_raw(reader) };
        drop(boxed)
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_close_write(writer: FileWriter) {
    if !writer.is_null() {
        let boxed = unsafe { Box::from_raw(writer) };
        drop(boxed)
    }
}

/// Calculate the number of bytes in the last error's error message **not**
/// including any trailing `null` characters.
#[no_mangle]
pub extern "C" fn last_error_length() -> c_int {
    LAST_ERROR.with(|prev| match *prev.borrow() {
        Some(ref err) => err.to_string().len() as c_int + 1,
        None => 0,
    })
}

/// Retrieve the most recent error, clearing it in the process.
pub fn take_last_error() -> Option<Error> {
    LAST_ERROR.with(|prev| prev.borrow_mut().take())
}

/// Write the most recent error message into a caller-provided buffer as a UTF-8
/// string, returning the number of bytes written.
///
/// # Note
///
/// This writes a **UTF-8** string into the buffer. Windows users may need to
/// convert it to a UTF-16 "unicode" afterwards.
///
/// If there are no recent errors then this returns `0` (because we wrote 0
/// bytes). `-1` is returned if there are any errors, for example when passed a
/// null pointer or a buffer of insufficient size.
#[no_mangle]
pub unsafe extern "C" fn last_error_message(buffer: *mut c_char, length: c_int) -> c_int {
    if buffer.is_null() {
        warn!("Null pointer passed into last_error_message() as the buffer");
        return -1;
    }

    let last_error = match take_last_error() {
        Some(err) => err,
        None => return 0,
    };

    let error_message = last_error.to_string();

    let buffer = slice::from_raw_parts_mut(buffer as *mut u8, length as usize);

    if error_message.len() >= buffer.len() {
        warn!("Buffer provided for writing the last error message is too small.");
        warn!(
            "Expected at least {} bytes but got {}",
            error_message.len() + 1,
            buffer.len()
        );
        return -1;
    }

    ptr::copy_nonoverlapping(
        error_message.as_ptr(),
        buffer.as_mut_ptr(),
        error_message.len(),
    );

    // Add a trailing null so people using the string as a `char *` don't
    // accidentally read into garbage.
    buffer[error_message.len()] = 0;

    error_message.len() as c_int
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BufferTooLong => write!(f, "Provided buffer is too long"),
            Error::InvalidPointer => write!(f, "Provided pointer is invalid"),
            Error::NonUtf8Path => write!(f, "Provided path is no valid UTF-8"),
            Error::BufferedFileErrors(BufferedFileErrors::AllFilesInvalidError) => {
                write!(f, "No valid file exists.")
            }
            Error::BufferedFileErrors(BufferedFileErrors::IoError(err)) => {
                write!(f, "Underlying IO Error: {}", err)
            }
        }
    }
}
