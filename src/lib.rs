//! Time Zone Information Format (TZif), RFC 8536

use std::io::{self, Read};

#[repr(transparent)]
#[derive(Copy, Clone)]
struct Bu32(u32);

impl From<Bu32> for u32 {
    fn from(big: Bu32) -> u32 {
        u32::from_be(big.0)
    }
}

impl From<u32> for Bu32 {
    fn from(native: u32) -> Bu32 {
        Bu32(u32::to_be(native))
    }
}

impl std::fmt::Debug for Bu32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", u32::from(*self))
    }
}

#[repr(packed)]
#[derive(Debug)]
struct Header {
    /// Must be the byte string b"TZif"
    magic: [u8; 4],

    /// Version. Either 0, b'2' or b'3'.
    ver: u8,

    _reserved: [u8; 15],

    /// Number of UT/local indicators contained in the data block.
    ///
    /// Must be either 0 or equal to [`typecnt`](Header::typecnt).
    isutcnt: Bu32,

    /// Number of standard/wall indicators contained in the data block.
    ///
    /// Must be either 0 or equal to [`typecnt`](Header::typecnt).
    isstdcnt: Bu32,

    /// Number of leap-second records contained in the data block.
    leapcnt: Bu32,

    /// Number of transition times contained in the data block.
    timecnt: Bu32,

    /// Number of local time type records contained in the data block.
    ///
    /// Must not be zero.
    typecnt: Bu32,

    /// Total number of bytes used by the set of time zone designations contained in the data
    /// block, including the triling NUL byte at the end of the last time zone designation.
    ///
    /// Must not be zero.
    charcnt: Bu32,
}

impl Header {
    pub fn from_array(bytes: [u8; 44]) -> Self {
        unsafe { std::mem::transmute(bytes) }
    }
}

#[derive(Debug, Default)]
pub struct TimeZoneInfo {
    pub version: u8,
    pub transition_times: Vec<i64>,
    pub transition_types: Vec<u8>,
    pub local_time_types: Vec<LocalTimeTypeRecord>,
    pub time_zone_designations: Vec<u8>,
    pub leap_second_records: Vec<(i64, i32)>,
    pub is_std: Vec<IsStd>,
    pub is_ut: Vec<IsUT>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum IsStd {
    Standard,
    Wall,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum IsUT {
    UT,
    Local,
}

#[derive(Debug)]
pub struct LocalTimeTypeRecord {
    pub ut_off_secs: i32,
    pub is_dst: bool,
    pub desig_idx: u8,
}

fn bogus<T, E>(inner: E) -> io::Result<T>
    where E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    Err(io::Error::new(io::ErrorKind::InvalidData, inner))
}

impl TimeZoneInfo {
    pub fn parse(mut reader: impl Read) -> io::Result<Self> {
        let v1_result = Self::parse_internal(&mut reader, true)?;
        if v1_result.version == 1 {
            return Ok(v1_result);
        }
        Self::parse_internal(&mut reader, false)
            .or(Ok(v1_result))
    }

    fn parse_internal(mut reader: impl Read, v1: bool) -> io::Result<Self> {
        let mut hbuf = [0u8; 44];
        reader.read_exact(&mut hbuf[..])?;
        let hdr = Header::from_array(hbuf);

        if &hdr.magic != b"TZif" {
            return bogus("unrecognized magic in header");
        }

        if hdr.isstdcnt.0 != 0 && hdr.isstdcnt.0 != hdr.typecnt.0 {
            return bogus("isstdcnt not zero or equal to typecnt");
        }
        if hdr.isutcnt.0 != 0 && hdr.isutcnt.0 != hdr.typecnt.0 {
            return bogus("isutcnt not zero or equal to typecnt");
        }

        let mut result = Self {
            version: match hdr.ver {
                0 => 1,
                b'2' => 2,
                b'3' => 3,
                _ => return bogus(format!("unsupported version {:#x}", hdr.ver)),
            },
            ..Self::default()
        };

        for _ in 0 .. hdr.timecnt.into() {
            let t = read_time(v1, &mut reader)?;
            result.transition_times.push(t);
        };

        result.transition_types.resize(u32::from(hdr.timecnt) as usize, 0);
        reader.read_exact(&mut result.transition_types)?;

        for _ in 0 .. hdr.typecnt.into() {
            let mut buf = [0u8; 4];
            reader.read_exact(&mut buf)?;
            let ut_off_secs = i32::from_be_bytes(buf);

            let mut isdst_idx = [0u8; 2];
            reader.read_exact(&mut isdst_idx)?;
            if !(0..=1).contains(&isdst_idx[0]) {
                return bogus("is_dst not zero or one");
            }

            let record = LocalTimeTypeRecord {
                ut_off_secs,
                is_dst: isdst_idx[0] == 1,
                desig_idx: isdst_idx[1],
            };
            result.local_time_types.push(record);
        }
        
        result.time_zone_designations.resize(u32::from(hdr.charcnt) as usize, 0);
        reader.read_exact(&mut result.time_zone_designations)?;

        for _ in 0 .. hdr.leapcnt.into() {
            let t = read_time(v1, &mut reader)?;
            let mut buf = [0u8; 4];
            reader.read_exact(&mut buf)?;
            let off = i32::from_be_bytes(buf);
            result.leap_second_records.push((t, off));
        }

        let mut buf = vec![0; u32::from(hdr.isstdcnt) as usize];
        reader.read_exact(&mut buf)?;
        for b in buf {
            result.is_std.push(match b {
                0 => IsStd::Wall,
                1 => IsStd::Standard,
                _ => return bogus("std/wall not zero or one"),
            });
        }

        buf = vec![0; u32::from(hdr.isutcnt) as usize];
        reader.read_exact(&mut buf)?;
        for b in buf {
            result.is_ut.push(match b {
                0 => IsUT::Local,
                1 => IsUT::UT,
                _ => return bogus("ut/local not zero or one"),
            });
        }

        for i in 0 .. result.is_std.len().max(result.is_ut.len()) {
            let is_std = result.is_std.get(i).unwrap_or(&IsStd::Wall);
            let is_ut = result.is_ut.get(i).unwrap_or(&IsUT::Local);
            if (is_std, is_ut) == (&IsStd::Wall, &IsUT::UT) {
                return bogus("transition times can't be universal + wall");
            }
        }

        for typ_idx in &result.transition_types {
            if *typ_idx as usize > result.local_time_types.len() {
                return bogus("one or more transition types out of range");
            }
        }

        Ok(result)
    }

    pub fn iter_transitions(&self) -> TransitionIterator<'_> {
        TransitionIterator {
            tzif: self,
            idx: 0,
        }
    }

    fn local_time_type(&self, idx: usize) -> LocalTimeType<'_> {
        let typ = &self.local_time_types[idx];

        let dstart = typ.desig_idx as usize;
        let mut dend = dstart;
        while let Some(b) = self.time_zone_designations.get(dend) {
            if *b == 0 {
                break;
            }
            dend += 1;
        }
        let desig = std::str::from_utf8(&self.time_zone_designations[dstart..dend]).unwrap();

        LocalTimeType {
            desig,
            ut_offset_secs: typ.ut_off_secs,
            is_dst: typ.is_dst,
        }
    }
}

pub struct TransitionIterator<'a> {
    tzif: &'a TimeZoneInfo,
    idx: usize,
}

impl<'a> Iterator for TransitionIterator<'a> {
    type Item = TimeTransition<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.tzif.transition_times.len() {
            return None;
        }

        let at_ts = self.tzif.transition_times[self.idx];
        let typ_idx = self.tzif.transition_types[self.idx];
        let local = self.tzif.local_time_type(typ_idx as usize);
        let at_time = match (self.tzif.is_std[typ_idx as usize], self.tzif.is_ut[typ_idx as usize]) {
            (IsStd::Standard, IsUT::UT) => Time::UT(at_ts),
            (IsStd::Standard, IsUT::Local) => Time::LocalStandard(at_ts),
            (IsStd::Wall, IsUT::UT) => panic!("transition time can't be wall+universal"),
            (IsStd::Wall, IsUT::Local) => Time::LocalWall(at_ts),
        };

        self.idx += 1;
        Some(TimeTransition { at_time, local })
    }
}

#[derive(Debug)]
pub struct TimeTransition<'a> {
    pub at_time: Time,
    pub local: LocalTimeType<'a>,
}

#[derive(Debug)]
pub struct LocalTimeType<'a> {
    pub desig: &'a str,
    pub ut_offset_secs: i32,
    pub is_dst: bool,
}

#[derive(Debug)]
pub enum Time {
    LocalWall(i64),
    LocalStandard(i64),
    UT(i64),
}

impl Time {
    pub fn to_ut(&self, local: &LocalTimeType<'_>) -> i64 {
        match self {
            Time::UT(t) => *t,
            Time::LocalStandard(t) => t + i64::from(local.ut_offset_secs),
            Time::LocalWall(t) => t + i64::from(local.ut_offset_secs) + if local.is_dst { 60*60 }  else { 0 },
        }
    }
}

fn read_time(v1: bool, mut reader: impl Read) -> io::Result<i64> {
    Ok(if v1 {
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        i64::from(i32::from_be_bytes(buf))
    } else {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        i64::from_be_bytes(buf)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_v1() {
        let bytes = [
            0x54, 0x5a, 0x69, 0x66,
            
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,

            0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x1b,
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x04,
        ];

        let hdr = Header::from_array(bytes);
        println!("{hdr:#?}");
        assert_eq!(b"TZif", &hdr.magic);
        assert_eq!(0, hdr.ver);
        assert_eq!(1u32, hdr.isutcnt.into());
        assert_eq!(1u32, hdr.isstdcnt.into());
        assert_eq!(27u32, hdr.leapcnt.into());
        assert_eq!(0u32, hdr.timecnt.into());
        assert_eq!(1u32, hdr.typecnt.into());
        assert_eq!(4u32, hdr.charcnt.into());
    }
}
