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

#[repr(C)]
pub struct FileReader(*mut BufferedFileReader<std::fs::File>);

#[repr(C)]
pub struct FileWriter(*mut BufferedFileWriter<std::fs::File>);

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
pub extern "C" fn open_read(path: *const c_char) -> FileReader {
    let path = unsafe { CStr::from_ptr(path) };
    let path = match path.to_str() {
        Ok(path) => path,
        Err(_err) => {
            // TODO Error handling in ffi
            return FileReader(ptr::null_mut());
        }
    };
    let path = PathBuf::from(path);

    let file = match BufferedFile::new(path) {
        Ok(file) => file,
        Err(_) => {
            // TODO Error handling in ffi
            return FileReader(ptr::null_mut());
        }
    };

    match file.read() {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let reference = std::boxed::Box::<_>::leak(boxed);
            FileReader(reference as *mut _)
        }
        Err(_) => FileReader(ptr::null_mut()),
    }
}

#[no_mangle]
pub extern "C" fn open_write(path: *const c_char) -> FileWriter {
    let path = unsafe { CStr::from_ptr(path) };
    let path = match path.to_str() {
        Ok(path) => path,
        Err(_err) => {
            // TODO Error handling in ffi
            return FileWriter(ptr::null_mut());
        }
    };
    let path = PathBuf::from(path);

    let file = match BufferedFile::new(path) {
        Ok(file) => file,
        Err(_) => {
            // TODO Error handling in ffi
            return FileWriter(ptr::null_mut());
        }
    };

    match file.write() {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let reference = std::boxed::Box::<_>::leak(boxed);
            FileWriter(reference as *mut _)
        }
        Err(_) => FileWriter(ptr::null_mut()),
    }
}

#[no_mangle]
pub extern "C" fn read(reader: FileReader, buffer: *mut u8, buffer_len: usize) -> i64 {
    if buffer_len > usize::try_from(i64::MAX).unwrap_or(buffer_len) {
        return ErrorCode::BufferTooLong.into();
    }

    if reader.0.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    if buffer.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    let reader = unsafe { &mut *reader.0 };
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, buffer_len) };
    match reader.read(buf) {
        Ok(amt) => i64::try_from(amt).expect("We checked the buffer size should fit into i64"),
        Err(err) => ErrorCode::from(err).into(),
    }
}

#[no_mangle]
pub extern "C" fn write(writer: FileWriter, buffer: *mut u8, buffer_len: usize) -> i64 {
    if buffer_len > usize::try_from(i64::MAX).unwrap_or(buffer_len) {
        return ErrorCode::BufferTooLong.into();
    }

    if writer.0.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    if buffer.is_null() {
        return ErrorCode::InvalidPointer.into();
    }

    let writer = unsafe { &mut *writer.0 };
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, buffer_len) };
    match writer.write(buf) {
        Ok(amt) => i64::try_from(amt).expect("We checked the buffer size should fit into i64"),
        Err(err) => ErrorCode::from(err).into(),
    }
}

#[no_mangle]
pub extern "C" fn close_read(reader: FileReader) {
    if !reader.0.is_null() {
        let boxed = unsafe { Box::from_raw(reader.0) };
        drop(boxed)
    }
}

#[no_mangle]
pub extern "C" fn close_write(writer: FileWriter) {
    if !writer.0.is_null() {
        let boxed = unsafe { Box::from_raw(writer.0) };
        drop(boxed)
    }
}
