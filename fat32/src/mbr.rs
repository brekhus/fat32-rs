use std::{fmt, io, mem, slice};

use traits::BlockDevice;

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct CHS {
    ignored_: [u8; 3],
}

#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct PartitionEntry {
    pub boot_indicator: u8,
    start_chs: CHS,
    pub partition_type: u8,
    end_chs: CHS,
    pub start_sector: u32,
    pub total_sectors: u32,
}

/// The master boot record (MBR).
#[repr(C, packed)]
pub struct MasterBootRecord {
    bootstrap_instr: [u8; 436],
    unique_disk_id: [u8; 10],
    pub part_entries: [PartitionEntry; 4],
    bootsector_signature: u16,
}

#[derive(Debug)]
pub enum Error {
    /// There was an I/O error while reading the MBR.
    Io(io::Error),
    /// Partiion `.0` (0-indexed) contains an invalid or unknown boot indicator.
    UnknownBootIndicator(u8),
    /// The MBR magic signature was invalid.
    BadSignature,
}

impl MasterBootRecord {
    /// Reads and returns the master boot record (MBR) from `device`.
    ///
    /// # Errors
    ///
    /// Returns `BadSignature` if the MBR contains an invalid magic signature.
    /// Returns `UnknownBootIndicator(n)` if partition `n` contains an invalid
    /// boot indicator. Returns `Io(err)` if the I/O error `err` occured while
    /// reading the MBR.
    pub fn from<T: BlockDevice>(mut device: T) -> Result<MasterBootRecord, Error> {
        assert_eq!(mem::size_of::<MasterBootRecord>(), 512);
        let mut mbr: MasterBootRecord = unsafe { mem::uninitialized() };
        {
            let mut mbr_as_buf =  unsafe {
                slice::from_raw_parts_mut(
                    &mut mbr as *mut MasterBootRecord as *mut u8, 
                    512)
            };
            if let Err(x) = device.read_sector(0, &mut mbr_as_buf) {
                return Err(Error::Io(x));
            }
        } 
        if mbr.bootsector_signature == 0xAA55 {
            for (i, ref entry) in mbr.part_entries.iter().enumerate() {
                match entry.boot_indicator {
                    0 | 0x80 => (),
                    _ => return Err(Error::UnknownBootIndicator(i as u8))
                };
            };
            // println!("mbr = {:#?}", mbr);
            Ok(mbr)
        } else {
            Err(Error::BadSignature)
        }
    }
}

impl fmt::Debug for MasterBootRecord {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FatEntry")
            .field("part_entries", &self.part_entries)
            .field("bootsector_signature", &self.bootsector_signature)
            .finish()
    }
}
