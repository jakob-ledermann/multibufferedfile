use std::{
    cmp::Ordering,
    fs::OpenOptions,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    thread::current,
};

use crc::{Crc, CRC_32_ISCSI};
use thiserror::Error;

const BUFFER_COUNT: u8 = 2;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Generation {
    Valid(u8),
    Invalid(u8),
    None,
}

impl Generation {
    pub fn is_valid(&self) -> bool {
        match self {
            Generation::Valid(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct BufferedFile {
    files: Vec<(std::path::PathBuf, Generation)>,
}

#[derive(Error, Debug)]
pub enum BufferedFileErrors {
    #[error("Error interacting with filesystem: '{0}")]
    IoError(#[from] std::io::Error),
    #[error("No valid file available")]
    AllFilesInvalidError,
}

enum FileCheckResult {
    Good { generation: Generation },
    ChecksumFailure,
}
const CRC: crc::Crc<u32> = Crc::<u32>::new(&CRC_32_ISCSI);

fn check_file(file: &Path) -> std::io::Result<FileCheckResult> {
    let mut file = std::fs::File::open(file)?;
    let mut digest = CRC.digest();
    let mut buf = [0u8; 8192];
    let mut valid = file.read(&mut buf)?;
    let read = &buf[..valid];
    let generation = read[0];
    digest.update(&read[1..read.len() - 4]);
    let mut potential_expected_crc32: u32 = u32::from_le_bytes(
        read[read.len() - 4..]
            .try_into()
            .expect("I should have valid bytes available"),
    );
    loop {
        valid = file.read(&mut buf)?;
        match valid {
            0 => {
                // File is finished
                return Ok(if digest.finalize() == potential_expected_crc32 {
                    FileCheckResult::Good {
                        generation: Generation::Valid(generation),
                    }
                } else {
                    FileCheckResult::ChecksumFailure
                });
            }
            x if x < 4 => {
                todo!("not enough data available for a potential crc32 checksum")
            }
            _ => {
                let read = &buf[..valid];
                let (data, pot_checksum) = read.split_at(read.len() - 4);
                potential_expected_crc32 = u32::from_le_bytes(
                    pot_checksum
                        .try_into()
                        .expect("there should be 4 u8 available"),
                );
                digest.update(&potential_expected_crc32.to_le_bytes());
                digest.update(data);
            }
        }
    }
}

pub use reader::*;

mod reader;

pub use writer::*;

mod writer;

impl BufferedFile {
    pub fn new(path: std::path::PathBuf) -> Result<Self, BufferedFileErrors> {
        let files = Self::find_files(&path)?;
        let files = files
            .into_iter()
            .flat_map(|f| match check_file(&f) {
                Ok(FileCheckResult::Good { generation }) => Ok((f, generation)),
                Ok(FileCheckResult::ChecksumFailure) => Ok((f, Generation::None)),
                Err(err) => match err.kind() {
                    ErrorKind::NotFound => Ok((f, Generation::None)),
                    _ => Err(err),
                },
            })
            .collect::<Vec<_>>();

        Ok(BufferedFile { files })
    }

    fn select_newest_valid(&self) -> Result<&Path, BufferedFileErrors> {
        let file = self
            .files
            .iter()
            .filter(|(_, gen)| gen.is_valid())
            .max_by_key(|(_, gen)| match gen {
                Generation::Valid(val) => *val,
                _ => 0,
            });

        match file {
            Some((file, _)) => Ok(file),
            None => Err(BufferedFileErrors::AllFilesInvalidError),
        }
    }

    pub fn read(self) -> Result<BufferedFileReader<std::fs::File>, BufferedFileErrors> {
        let file = self.select_newest_valid()?;
        let mut file = OpenOptions::new().read(true).open(file)?;
        file.seek(SeekFrom::Start(1))?;
        let usable_file_size = file.metadata()?.len().saturating_sub(4);
        Ok(BufferedFileReader::new(file, usable_file_size))
    }

    pub fn write(self) -> Result<BufferedFileWriter<std::fs::File>, BufferedFileErrors> {
        let file = self
            .files
            .iter()
            .min_by_key(|(_, gen)| match gen {
                Generation::Valid(val) => *val,
                _ => 0u8,
            })
            .expect("Files should contain at least one value");

        let current_generation = self
            .files
            .iter()
            .map(|(_, gen)| match gen {
                Generation::Valid(val) => *val,
                _ => 0u8,
            })
            .max()
            .expect("Files should contain at least one value");

        let mut target_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file.0)?;
        target_file.write_all(&[current_generation.wrapping_add(1)])?;

        Ok(BufferedFileWriter::new(target_file))
    }

    fn find_files(path: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
        let stem = path
            .file_name()
            .expect("provided path should be a valid file path");
        let ancestor = path
            .parent()
            .expect("provided path should be a valid file path");

        let mut result = Vec::with_capacity(BUFFER_COUNT.into());
        for i in 1..=BUFFER_COUNT {
            let mut file = ancestor.to_path_buf();
            let mut file_name = stem.to_os_string();
            file_name.push(format!(".{i}"));
            file.push(file_name);

            result.push(file);
        }
        Ok(result)
    }
}

fn wrapping_cmp(a: u8, b: u8) -> Ordering {
    match a.wrapping_sub(b) {
        0 => Ordering::Equal,
        x if x < 128 => Ordering::Less,
        _ => Ordering::Greater,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::{tests::utils::TempDir, BufferedFile, BufferedFileErrors};

    #[test]
    fn new_file_gives_error_on_read() {
        let dir = TempDir::new();
        let file = dir.path().join("data-file.txt");
        let managed_file = BufferedFile::new(file)
            .expect("It should be possible to create for not yet existing files.");

        let reader = managed_file.read();
        assert!(
            matches!(reader, Err(BufferedFileErrors::AllFilesInvalidError)),
            "Reader is a {reader:?}. Expected an Err(BufferedFileErrors::AllFilesInvalidError)"
        );
    }

    #[test]
    fn can_write_new_file() {
        let dir = TempDir::new();
        let file = dir.path().join("data-file.txt");
        let managed_file = BufferedFile::new(file)
            .expect("It should be possible to create for not yet existing files.");

        let mut writer = managed_file
            .write()
            .expect("A new file should be writeable");

        writer
            .write_all(b"Hello World")
            .expect("Can not write into the file");

        drop(writer);

        let expected_file = dir.path().join("data-file.txt.1");
        assert!(expected_file.exists());
    }

    mod utils {
        use std::{
            env, fs,
            path::{Path, PathBuf},
        };

        #[derive(Debug)]
        pub struct TempDir(PathBuf);

        impl Drop for TempDir {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }

        impl TempDir {
            /// Create a new empty temporary directory under the system's configured
            /// temporary directory.
            pub fn new() -> TempDir {
                use std::sync::atomic::{AtomicUsize, Ordering};

                static TRIES: usize = 100;
                #[allow(deprecated)]
                static COUNTER: AtomicUsize = AtomicUsize::new(0);

                let tmpdir = env::temp_dir();
                for _ in 0..TRIES {
                    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
                    let path = tmpdir.join("rust-walkdir").join(count.to_string());
                    if path.is_dir() {
                        continue;
                    }
                    fs::create_dir_all(&path)
                        .map_err(|e| panic!("failed to create {}: {}", path.display(), e))
                        .unwrap();
                    return TempDir(path);
                }
                panic!("failed to create temp dir after {} tries", TRIES)
            }

            /// Return the underlying path to this temporary directory.
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
    }
}
