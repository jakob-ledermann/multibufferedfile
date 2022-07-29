use std::io::{ErrorKind, Read, Write};
use std::ptr;
use std::{ffi::CStr, os::raw::c_char, path::PathBuf};

use crate::{BufferedFile, BufferedFileReader, BufferedFileWriter};

#[repr(i64)]
pub enum ErrorCode {
    Success = 0,
    NonUtf8Path = -200,
    BufferTooLong = -201,
    InvalidPointer = -202,
    FileNotFound = -1,
    UnknownIoError = -3,
}

pub type FileReader = *mut BufferedFileReader<std::fs::File>;

pub type FileWriter = *mut BufferedFileWriter<std::fs::File>;

impl From<ErrorCode> for i64 {
    fn from(other: ErrorCode) -> Self {
        other as i64
    }
}

impl From<std::io::Error> for ErrorCode {
    fn from(other: std::io::Error) -> Self {
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
            return ptr::null_mut();
        }
    };
    let path = PathBuf::from(path);

    let file = match BufferedFile::new(path) {
        Ok(file) => file,
        Err(_) => {
            // TODO Error handling in ffi
            return ptr::null_mut();
        }
    };

    match file.read() {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let reference = std::boxed::Box::<_>::leak(boxed);
            reference as *mut _
        }
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_open_write(path: *const c_char) -> FileWriter {
    let path = unsafe { CStr::from_ptr(path) };
    let path = match path.to_str() {
        Ok(path) => path,
        Err(_err) => {
            // TODO Error handling in ffi
            return ptr::null_mut();
        }
    };
    let path = PathBuf::from(path);

    let file = match BufferedFile::new(path) {
        Ok(file) => file,
        Err(_) => {
            // TODO Error handling in ffi
            return ptr::null_mut();
        }
    };

    match file.write() {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let reference = std::boxed::Box::<_>::leak(boxed);
            reference as *mut _
        }
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_read(reader: FileReader, buffer: *mut u8, buffer_len: usize) -> i64 {
    if buffer_len > usize::try_from(i64::MAX).unwrap_or(buffer_len) {
        return ErrorCode::BufferTooLong.into();
    }

    if reader.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    if buffer.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    let reader = unsafe { &mut *reader };
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, buffer_len) };
    match reader.read(buf) {
        Ok(amt) => i64::try_from(amt).expect("We checked the buffer size should fit into i64"),
        Err(err) => ErrorCode::from(err).into(),
    }
}

#[no_mangle]
pub extern "C" fn bufferedfile_write(
    writer: FileWriter,
    buffer: *mut u8,
    buffer_len: usize,
) -> i64 {
    if buffer_len > usize::try_from(i64::MAX).unwrap_or(buffer_len) {
        return ErrorCode::BufferTooLong.into();
    }

    if writer.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    if buffer.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    let writer = unsafe { &mut *writer };
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, buffer_len) };
    match writer.write(buf) {
        Ok(amt) => i64::try_from(amt).expect("We checked the buffer size should fit into i64"),
        Err(err) => ErrorCode::from(err).into(),
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
