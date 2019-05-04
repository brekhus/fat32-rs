use std::{fmt, mem, slice};

use traits::BlockDevice;
use vfat::Error;

#[repr(C, packed)]
pub struct BiosParameterBlock {
    pub bootcode_trampoline: [u8; 3],
    pub oem_id: [u8; 8],
    pub sector_bytes: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub max_dirent_count: u16,
    pub logical_sectors_small: u16,
    pub media_descriptor_type: u8,
    pub sectors_per_fat_obsolete: u16,
    pub sectors_per_track: u16,
    pub heads: u16,
    pub hidden_sectors: u32,
    pub logical_sectors_large: u32,
    pub sectors_per_fat: u32,
    pub flags: u16,
    pub fat_version_number: u16,
    pub root_start_cluster: u32,
    pub fsinfo_sector: u16,
    pub backup_boot_sector: u16,
    pub _reserved: [u8; 12],
    pub drive_number: u8,
    pub _reserved2: u8,
    pub signature: u8,
    pub volume_serial: u32,
    pub volume_label: [u8; 11],
    pub system_identifier: [u8; 8],
    pub bootcode: [u8; 420],
    pub partition_signature: u16,
}

impl BiosParameterBlock {
    /// Reads the FAT32 extended BIOS parameter block from sector `sector` of
    /// device `device`.
    ///
    /// # Errors
    ///
    /// If the EBPB signature is invalid, returns an error of `BadSignature`.
    pub fn from<T: BlockDevice>(
        mut device: T,
        sector: u64
    ) -> Result<BiosParameterBlock, Error> {
        assert_eq!(mem::size_of::<BiosParameterBlock>(), 512);
        let mut bpb: BiosParameterBlock = unsafe { mem::uninitialized() };
        {
            let mut bpb_as_buf = unsafe {
                slice::from_raw_parts_mut(
                    &mut bpb as *mut BiosParameterBlock as *mut u8, 
                    512)
            };
            device.read_sector(sector, &mut bpb_as_buf)?;
        }
        if bpb.partition_signature == 0xAA55 {
            // println!("bpb = {:#?}", bpb);
            Ok(bpb)
        } else {
            Err(Error::BadSignature)
        }
    }
}

impl fmt::Debug for BiosParameterBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BiosParameterBlock")
            .field("sector_bytes", &self.sector_bytes)
            .field("reserved_sectors", &self.reserved_sectors)
            .field("fat_count", &self.fat_count)
            .field("logical_sectors_small", &self.logical_sectors_small)
            .field("logical_sectors_large", &self.logical_sectors_large)
            .field("sectors_per_fat", &self.sectors_per_fat)
            .field("flags", &self.flags)
            .field("root_start_cluster", &self.root_start_cluster)
            .field("drive_number", &self.drive_number)
            .field("partition_signature", &self.partition_signature)
            .finish()
    }
}
