//! Account only actual inspection reads; seek distances stay inside the verified snapshot.
use super::unsupported;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailure, SendFailureKind};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};

pub(super) const READ_LIMIT: usize = 65_536;

pub(super) struct Reader<'a> {
    file: &'a mut File,
    len: u64,
    position: u64,
    inspected: usize,
    deadline: SendDeadline,
}
impl<'a> Reader<'a> {
    // The caller has rewound the file and retains responsibility for restoring its cursor.
    pub(super) fn new(file: &'a mut File, len: u64, deadline: SendDeadline) -> Self {
        Self {
            file,
            len,
            position: 0,
            inspected: 0,
            deadline,
        }
    }
    pub(super) fn check(&self) -> Result<(), SendFailure> {
        if self.deadline.expired() {
            Err(SendFailure::not_sent(SendFailureKind::Timeout, None))
        } else {
            Ok(())
        }
    }
    pub(super) fn end(&self, size: u64) -> Result<u64, SendFailure> {
        self.position
            .checked_add(size)
            .filter(|end| *end <= self.len)
            .ok_or_else(unsupported)
    }
    pub(super) fn read<const N: usize>(&mut self) -> Result<[u8; N], SendFailure> {
        let mut bytes = [0; N];
        self.read_into(&mut bytes)?;
        Ok(bytes)
    }
    pub(super) fn read_into(&mut self, mut bytes: &mut [u8]) -> Result<(), SendFailure> {
        self.check()?;
        self.end(bytes.len() as u64)?;
        if bytes.len() > READ_LIMIT - self.inspected {
            return Err(unsupported());
        }
        while !bytes.is_empty() {
            self.check()?;
            let count = match self.file.read(bytes) {
                Ok(0) => return Err(unsupported()),
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(unsupported()),
            };
            self.inspected += count;
            self.position += count as u64;
            bytes = &mut bytes[count..];
            self.check()?;
        }
        Ok(())
    }
    pub(super) fn seek(&mut self, position: u64) -> Result<(), SendFailure> {
        self.check()?;
        if position > self.len {
            return Err(unsupported());
        }
        self.file
            .seek(SeekFrom::Start(position))
            .map_err(|_| unsupported())?;
        self.position = position;
        self.check()
    }
}
