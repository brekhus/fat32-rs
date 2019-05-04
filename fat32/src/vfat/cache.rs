use std::{io, fmt};
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use std::io::Write;

use traits::BlockDevice;

#[derive(Debug)]
struct CacheEntry {
    data: Vec<u8>,
    dirty: bool
}

#[derive(Debug)]
pub struct Partition {
    /// The physical sector where the partition begins.
    pub start: u64,
    /// The size, in bytes, of a logical sector in the partition.
    pub sector_size: u64
}

pub struct CachedDevice {
    device: Box<BlockDevice>,
    cache: HashMap<u64, CacheEntry>,
    partition: Partition
}

impl CachedDevice {
    /// Creates a new `CachedDevice` that transparently caches sectors from
    /// `device` and maps physical sectors to logical sectors inside of
    /// `partition`. All reads and writes from `CacheDevice` are performed on
    /// in-memory caches.
    ///
    /// The `partition` parameter determines the size of a logical sector and
    /// where logical sectors begin. An access to a sector `n` _before_
    /// `partition.start` is made to physical sector `n`. Cached sectors before
    /// `partition.start` are the size of a physical sector. An access to a
    /// sector `n` at or after `partition.start` is made to the _logical_ sector
    /// `n - partition.start`. Cached sectors at or after `partition.start` are
    /// the size of a logical sector, `partition.sector_size`.
    ///
    /// `partition.sector_size` must be an integer multiple of
    /// `device.sector_size()`.
    ///
    /// # Panics
    ///
    /// Panics if the partition's sector size is < the device's sector size.
    pub fn new<T>(device: T, partition: Partition) -> CachedDevice
        where T: BlockDevice + 'static
    {
        assert!(partition.sector_size >= device.sector_size());

        CachedDevice {
            device: Box::new(device),
            cache: HashMap::new(),
            partition: partition
        }
    }

    /// Maps a user's request for a sector `virt` to the physical sector and
    /// number of physical sectors required to access `virt`.
    fn virtual_to_physical(&self, virt: u64) -> (u64, u64) {
        if self.device.sector_size() == self.partition.sector_size {
            (virt, 1)
        } else if virt < self.partition.start {
            (virt, 1)
        } else {
            let factor = self.partition.sector_size / self.device.sector_size();
            let logical_offset = virt - self.partition.start;
            let physical_offset = logical_offset * factor;
            let physical_sector = self.partition.start + physical_offset;
            (physical_sector, factor)
        }
    }

    /// Returns a mutable reference to the cached sector `sector`. If the sector
    /// is not already cached, the sector is first read from the disk.
    ///
    /// The sector is marked dirty as a result of calling this method as it is
    /// presumed that the sector will be written to. If this is not intended,
    /// use `get()` instead.
    ///
    /// # Errors
    ///
    /// Returns an error if there is an error reading the sector from the disk.
    pub fn get_mut(&mut self, sector: u64) -> io::Result<&mut [u8]> {
        self.get_internal(sector, true)

    }

    /// Returns a reference to the cached sector `sector`. If the sector is not
    /// already cached, the sector is first read from the disk.
    ///
    /// # Errors
    ///
    /// Returns an error if there is an error reading the sector from the disk.
    pub fn get(&mut self, sector: u64) -> io::Result<&[u8]> {
        Ok(&*self.get_internal(sector, false)?)
    }


    fn get_internal(&mut self, sector: u64, dirty: bool) -> io::Result<&mut [u8]> {
        let (phys_sector, count) = { self.virtual_to_physical(sector + self.partition.start) };
        // println!("logical_sector={:} phys_sector_offset={:} count={:}", sector, phys_sector, count);
        let entry = self.cache.entry(sector);
        match entry {
            Entry::Occupied(oe) => {
                let mut cache_entry = oe.into_mut();
                if dirty {
                    cache_entry.dirty = true;
                }
                return Ok(&mut cache_entry.data);
            },
            Entry::Vacant(ve) => {
                let mut data = Vec::with_capacity((count * self.device.sector_size()) as usize);
                for i in phys_sector..(phys_sector + count) {
                    self.device.read_all_sector(i, &mut data)?;
                }
                let mut cache_entry = ve.insert(CacheEntry { data: data, dirty: dirty });
                return Ok(&mut cache_entry.data);
            }
        }
    }
}

impl BlockDevice for CachedDevice {
    fn read_sector(&mut self, n: u64, mut buf: &mut [u8]) -> io::Result<usize> {
        if let Some(entry) = self.cache.get(&n) {
            buf.write(&entry.data)
        } else {
             Err(io::Error::new(io::ErrorKind::NotFound, "sector not available"))
        }
    }

    fn write_sector(&mut self, n: u64, buf: &[u8]) -> io::Result<usize> {
        let sector_size = { self.sector_size() as usize };
        if let Some(entry) = self.cache.get_mut(&n) {
            if buf.len() < sector_size {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "write too small"))
            } else {
                entry.dirty = true;
                entry.data.write(buf)
            }
        } else {
             Err(io::Error::new(io::ErrorKind::NotFound, "sector not available"))
        }
    }
}

// FIXME: Implement `BlockDevice` for `CacheDevice`. The `read_sector` and
// `write_sector` methods should only read/write from/to cached sectors.

impl fmt::Debug for CachedDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CachedDevice")
            .field("device", &"<block device>")
            .field("cache", &self.cache.keys())
            .field("partition", &self.partition)
            .finish()
    }
}
