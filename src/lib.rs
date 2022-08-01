use std::{
    cmp::Ordering,
    fs::OpenOptions,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crc::{Crc, CRC_32_BZIP2};
use thiserror::Error;

/// The number of parallel buffers, that exist at one point in time.
const BUFFER_COUNT: u8 = 2;

/// Describes the Generation of a stored file
///
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Generation {
    /// The generation of a valid file with the value of the generation
    Valid(u8),
    /// Marker for files which are either invalid or do not yet exist
    None,
}

impl Generation {
    /// Checks if the generation is valid
    pub fn is_valid(&self) -> bool {
        matches!(self, Generation::Valid(_))
    }
}

/// A double buffered File is represented here. It can be opened for either read or write access.
#[derive(Debug, PartialEq)]
pub struct BufferedFile {
    files: Vec<(std::path::PathBuf, Generation)>,
}

/// The definition of Errors of this library
#[derive(Error, Debug)]
pub enum BufferedFileErrors {
    /// The underlying filesystem reported an error
    #[error("Error interacting with filesystem: '{0}")]
    IoError(#[from] std::io::Error),
    /// Either no files exist, or all existing files are invalid
    #[error("No valid file available")]
    AllFilesInvalidError,
}

enum FileCheckResult {
    Good { generation: Generation },
    ChecksumFailure,
}

/// Stores and defines the used CRC algorithm for the checksums of the files
const CRC: crc::Crc<u32> = Crc::<u32>::new(&CRC_32_BZIP2);

pub use reader::*;

mod reader;

pub use writer::*;

mod writer;

mod ffi;

fn check_file(file: &Path) -> std::io::Result<FileCheckResult> {
    let mut file = std::fs::File::open(file)?;
    let mut digest = CRC.digest();
    let mut buf = [0u8; 8192];
    let mut valid = file.read(&mut buf)?;
    if valid < 5 {
        return Ok(FileCheckResult::ChecksumFailure);
    }
    let read = &buf[..valid];
    let generation = read[0];
    digest.update(&read[1..read.len().saturating_sub(4)]);
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

impl BufferedFile {
    /// Creates a representation of the managed file and scans all underlying files for their validity and generation.
    ///
    /// # Arguments
    /// * `path` - the path representing the desired file (this file does not exist on the filesystem)
    ///            The backing files are stored with a suffix of .1 and .2 respectively.
    ///
    /// # Example
    ///
    /// ```
    /// use multibufferedfile::BufferedFile;
    ///
    /// let file = BufferedFile::new("file.txt");
    /// assert!(file.is_ok());
    /// ```
    pub fn new(path: impl AsRef<Path>) -> Result<Self, BufferedFileErrors> {
        let files = Self::find_files(path);
        let files = files
            .into_iter()
            .flat_map(|f| match check_file(&f) {
                Ok(FileCheckResult::Good { generation }) => Ok((f, generation)),
                Ok(FileCheckResult::ChecksumFailure) => Ok((f, Generation::None)),
                Err(err) if err.kind() == ErrorKind::NotFound => Ok((f, Generation::None)),
                Err(err) => Err(err),
            })
            .collect::<Vec<_>>();

        Ok(BufferedFile { files })
    }

    /// selects the newest valid backing file
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

    ///
    /// Opens the managed file for read-only access
    pub fn read(self) -> Result<BufferedFileReader<std::fs::File>, BufferedFileErrors> {
        let file = self.select_newest_valid()?;
        let mut file = OpenOptions::new().read(true).open(file)?;
        file.seek(SeekFrom::Start(1))?;
        let usable_file_size = file.metadata()?.len().saturating_sub(5);
        Ok(BufferedFileReader::new(file, usable_file_size))
    }

    ///
    /// Opens the managed file for write access
    ///
    pub fn write(self) -> Result<BufferedFileWriter<std::fs::File>, BufferedFileErrors> {
        let file = self
            .files
            .iter()
            .min_by(|(_, a), (_, b)| match (a, b) {
                (Generation::Valid(a), Generation::Valid(b)) => wrapping_cmp(*a, *b),
                (Generation::None, Generation::None) => Ordering::Equal,
                (Generation::None, _) => Ordering::Less,
                (_, Generation::None) => Ordering::Greater,
            })
            .expect("Files should contain at least one value");

        let current_generation = self
            .files
            .iter()
            .map(|(_, gen)| match gen {
                Generation::Valid(val) => *val,
                _ => 0u8,
            })
            .max_by(|&a, &b| wrapping_cmp(a, b))
            .expect("Files should contain at least one value");

        let mut target_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file.0)?;
        target_file.write_all(&[current_generation.wrapping_add(1)])?;

        Ok(BufferedFileWriter::new(target_file))
    }

    fn find_files(path: impl AsRef<Path>) -> Vec<PathBuf> {
        let stem = path
            .as_ref()
            .file_name()
            .expect("provided path should be a valid file path");
        let ancestor = path
            .as_ref()
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
        result
    }
}

///
/// helps comparing the generations with wrapping behaviour (assumes increments of 1)
fn wrapping_cmp(a: u8, b: u8) -> Ordering {
    match a.wrapping_sub(b) {
        0 => Ordering::Equal,
        x if x < 128 => Ordering::Greater,
        _ => Ordering::Less,
    }
}

/// Provides tests for the helper function `wrapping_cmp`
#[test]
fn wrapping_cmp_test() {
    assert_eq!(wrapping_cmp(0, 0), Ordering::Equal);
    assert_eq!(wrapping_cmp(1, 1), Ordering::Equal);
    assert_eq!(wrapping_cmp(0, 1), Ordering::Less);
    assert_eq!(wrapping_cmp(1, 0), Ordering::Greater);
    assert_eq!(wrapping_cmp(255, 0), Ordering::Less);
    assert_eq!(wrapping_cmp(0, 255), Ordering::Greater);
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        ops::BitAnd,
    };

    use crate::{tests::utils::TempDir, BufferedFile, BufferedFileErrors};

    #[test]
    fn new_file_gives_error_on_read() {
        let dir = TempDir::new();
        let file = dir.path().join("data-file.txt");
        let managed_file = BufferedFile::new(&file)
            .expect("It should be possible to create for not yet existing files.");

        let reader = managed_file.read();
        assert!(
            matches!(reader, Err(BufferedFileErrors::AllFilesInvalidError)),
            "Reader is a {reader:?}. Expected an Err(BufferedFileErrors::AllFilesInvalidError)"
        );
    }

    #[test]
    fn can_read_a_written_file() {
        let dir = TempDir::new();
        let file = dir.path().join("data-file.txt");

        let managed_file = BufferedFile::new(&file)
            .expect("It should be possible to create for not yet existing files.");
        let mut writer = managed_file.write().expect("Can not write the file");
        writer
            .write_all(b"Hello World")
            .expect("Should be able to write");
        drop(writer);

        let mut reader = BufferedFile::new(&file)
            .expect("Can not find files")
            .read()
            .expect("Can not read the file");

        let mut contents = Vec::new();
        reader
            .read_to_end(&mut contents)
            .expect("Error reading from file");

        assert_eq!(contents.as_slice(), b"Hello World")
    }

    #[test]
    fn can_write_new_file() {
        let dir = TempDir::new();
        let file = dir.path().join("data-file.txt");

        let mut expected_generation: u8 = 0;
        for i in 1..300 {
            let managed_file = BufferedFile::new(&file)
                .expect("It should be possible to create for not yet existing files.");

            let mut writer = managed_file
                .write()
                .expect("A new file should be writeable");

            writer
                .write_all(b"Hello World")
                .expect("Can not write into the file");

            drop(writer);

            expected_generation = expected_generation.wrapping_add(1u8);
            let file_number = if i.bitand(1) > 0 { 1 } else { 2 };
            let expected_file = dir.path().join(format!("data-file.txt.{file_number}"));
            assert!(
                expected_file.exists(),
                "The file {expected_file:?} does not exist"
            );

            let mut contents = Vec::new();
            let mut file = std::fs::File::open(expected_file).expect("Could not open File");
            file.read_to_end(&mut contents)
                .expect("Could not verify written file");

            assert_eq!(
                contents.as_slice()[0],
                expected_generation,
                "Expected generation {expected_generation} in run {i}"
            );
            assert_eq!(&contents.as_slice()[1..], b"Hello World\xDA\x89\x5C\x06")
        }
    }

    #[test]
    fn can_write_empty_file() {
        let dir = TempDir::new();
        let file = dir.path().join("data-file.txt");

        let managed_file = BufferedFile::new(&file)
            .expect("It should be possible to create for not yet existing files.");

        let mut writer = managed_file
            .write()
            .expect("A new file should be writeable");

        writer.write_all(b"").expect("Can not write into the file");

        drop(writer);

        let expected_generation = 1;
        let file_number = 1;
        let expected_file = dir.path().join(format!("data-file.txt.{file_number}"));
        assert!(
            expected_file.exists(),
            "The file {expected_file:?} does not exist"
        );

        let mut contents = Vec::new();
        let mut file = std::fs::File::open(expected_file).expect("Could not open File");
        file.read_to_end(&mut contents)
            .expect("Could not verify written file");

        assert_eq!(
            contents.as_slice()[0],
            expected_generation,
            "Expected generation {expected_generation}"
        );
        assert_eq!(&contents.as_slice()[1..], b"\x00\x00\x00\x00")
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
