use std::fmt;

use traits;

/// A date as represented in FAT32 on-disk structures.
#[repr(C, packed)]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct Date(pub u16);

/// Time as represented in FAT32 on-disk structures.
#[repr(C, packed)]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct Time(pub u16);

/// File attributes as represented in FAT32 on-disk structures.
#[repr(C, packed)]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct Attributes(pub u8);

/// A structure containing a date and time.
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct Timestamp {
    pub time: Time,
    pub date: Date,
}

/// Metadata for a directory entry.
#[derive(Default, Debug, Clone)]
pub struct Metadata {
    pub attribs: Attributes,
    pub created: Timestamp,
    pub accessed: Date,
    pub modified: Timestamp
}

impl traits::Timestamp for Timestamp {
    fn year(&self) -> usize {
        (1980 + (self.date.0 >> 9)) as usize
    }

    fn month(&self) -> u8 {
        ((self.date.0 >> 5) & 0x0F) as u8
    }

    fn day(&self) -> u8 {
        (self.date.0 & 0x1F) as u8
    }

    fn hour(&self) -> u8 {
        ((self.time.0 >> 11) & 0x3F) as u8
    }

    fn minute(&self) -> u8 {
        ((self.time.0 >> 5) & 0x3F) as u8
    }

    fn second(&self) -> u8 {
        ((self.time.0 & 0x1F) * 2) as u8
    }
}


impl traits::Metadata for Metadata {
    type Timestamp = Timestamp;
    fn read_only(&self) -> bool {
        (self.attribs.0 & 0x01) != 0
    }

    fn hidden(&self) -> bool {
        (self.attribs.0 & 0x02) != 0
    }

    fn created(&self) -> Self::Timestamp {
        self.created
    }

    fn accessed(&self) -> Self::Timestamp {
        Timestamp { date: self.accessed, time: Time(0) }
    }

    fn modified(&self) -> Self::Timestamp {
        self.modified
    }
}
impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use traits::Timestamp;
        write!(f, 
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year(), self.month(), self.day(),
            self.hour(), self.minute(), self.second())
    }
}

impl fmt::Display for Metadata {

    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use traits::Metadata;
        if self.read_only() {
            write!(f, "+ro ")?;
        };

        if self.hidden() {
            write!(f, "+hidden ")?;
        }
        write!(f, "ctime={} atime={} mtime={}", 
            self.created(), self.accessed(), self.modified())
    }

}