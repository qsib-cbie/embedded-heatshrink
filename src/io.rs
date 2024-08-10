#[derive(Debug, PartialEq)]
pub enum ReadResult<'r> {
    /// The operation was successful.
    /// Contains a reference to the populated bytes.
    Ok(&'r mut [u8]),
    /// The operation was not successful and cannot be polled again.
    Error,
}

///
/// A trait for feeding data into a Heatshrink encoder like Read,
/// but available in no_std environments.
///
pub trait Readable {
    fn read<'r>(&self, buf: &'r mut [u8]) -> ReadResult<'r>;
}

impl<'a> Readable for &'a [u8] {
    fn read<'r>(&self, buf: &'r mut [u8]) -> ReadResult<'r> {
        let len = core::cmp::min(buf.len(), self.len());
        let buf = &mut buf[..len];
        buf.copy_from_slice(&self[..len]);
        ReadResult::Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_read_all_bytes_slice() {
        let data = [1, 2, 3, 4, 5];
        let mut buf = [0; 5];
        let slice = &data[..];
        let res = slice.read(&mut buf);
        assert_eq!(res, ReadResult::Ok(&mut [1, 2, 3, 4, 5]));
    }

    fn read_one<'a, 'b>(
        slice: &'a [u8],
        buf: &'b mut [u8],
        assertion: impl Fn(&'b [u8]),
    ) -> &'a [u8] {
        let buf = match slice.read(buf) {
            ReadResult::Ok(buf) => buf,
            e => panic!("Expected Ok, got {:?}", e),
        };
        assertion(buf);
        &slice[buf.len()..]
    }

    #[test]
    fn can_read_one_byte_at_a_time_slice() {
        let data = [1, 2, 3, 4, 5];
        let mut buf = [0; 1];
        let buf = &mut buf;
        let mut slice = &data[..];
        for i in 1..=5 {
            slice = read_one(slice, buf, |buf| assert_eq!(buf, &[i]));
        }
    }

    #[test]
    fn can_read_into_zero_bytes() {
        let data = [1, 2, 3, 4, 5];
        let mut buf = [0; 0];
        let slice = &data[..];
        let res = slice.read(&mut buf);
        assert_eq!(res, ReadResult::Ok(&mut []));
    }

    #[test]
    fn can_read_from_zero_bytes() {
        let data = [];
        let mut buf = [0; 5];
        let slice = &data[..];
        let res = slice.read(&mut buf);
        assert_eq!(res, ReadResult::Ok(&mut []));
    }

    #[test]
    fn can_read_from_vec() {
        let data = vec![1u8, 2, 3, 4, 5];
        let mut buf = [0; 5];
        let res = data.as_slice().read(&mut buf);
        assert_eq!(res, ReadResult::Ok(&mut [1, 2, 3, 4, 5]));
    }
}
