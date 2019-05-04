use std::io::Write;
use std::io;
use std::mem::size_of;
use std::ops::Range;
use std::path::{Path, Component};

use mbr::MasterBootRecord;
use vfat::{Shared, Cluster, File, Dir, Entry, FatEntry, Error, Status};
use vfat::{Metadata, Attributes, Date, Time, Timestamp};
use vfat::{BiosParameterBlock, CachedDevice, Partition};
use traits::{FileSystem, BlockDevice};

#[derive(Debug)]
pub struct VFat {
    device: CachedDevice,
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    sectors_per_fat: u32,
    fat_start_sector: u64,
    data_start_sector: u64,
    data_sectors: u64,
    root_dir_cluster: Cluster,
}

const ROOT_NAME: &str = "";
const ROOT_MD: Metadata = Metadata { 
    attribs: Attributes(0),
    created: Timestamp { date: Date(0), time: Time(0) },
    accessed: Date(0),
    modified: Timestamp { date: Date(0), time: Time(0) },
};

impl VFat {
    pub fn from<T>(mut device: T) -> Result<Shared<VFat>, Error>
        where T: BlockDevice + 'static
    {
        let mbr = MasterBootRecord::from(&mut device)?;
        let part_start = {
            let boot_part_ent = mbr.part_entries.iter().nth(0);
            match boot_part_ent {
                Some(entry) => entry.start_sector as u64,
                None => return Err(Error::NotFound),
            }
        };

        let bpb = BiosParameterBlock::from(&mut device, part_start)?;
        let part = Partition { start: part_start, sector_size: bpb.sector_bytes as u64 };
        let part_device = CachedDevice::new(device, part);
        let data_start_sector = (bpb.reserved_sectors as u64) 
                + ((bpb.sectors_per_fat as u64) * (bpb.fat_count as u64));

        let logical_sectors = if bpb.logical_sectors_small != 0 {
            bpb.logical_sectors_small as u64
        } else {
            bpb.logical_sectors_large as u64
        };


        Ok(Shared::new(VFat { 
            device: part_device,
            bytes_per_sector: bpb.sector_bytes as u16,
            sectors_per_cluster: bpb.sectors_per_cluster as u8,
            sectors_per_fat: bpb.sectors_per_fat as u32,
            fat_start_sector: bpb.reserved_sectors as u64,
            data_start_sector: data_start_sector,
            root_dir_cluster: Cluster::from(bpb.root_start_cluster),
            data_sectors: logical_sectors - data_start_sector,
        }))
    }

    fn coords(&self, cluster: Cluster, offset: usize) -> (Range<u64>, usize) {
        let cluster_start_sector = self.data_start_sector + (cluster.data_offset() * (self.sectors_per_cluster as u64));
        let start_sector = cluster_start_sector + ((offset / (self.bytes_per_sector as usize)) as u64);
        let end_sector = cluster_start_sector + (self.sectors_per_cluster as u64);
        let sector_offset = offset % (self.bytes_per_sector as usize);
        (start_sector..end_sector, sector_offset)
    }


    pub fn read_cluster(
        &mut self,
        cluster: Cluster,
        offset: usize,
        mut buf: &mut [u8]
    ) -> io::Result<usize> {
        assert!(offset < (self.sectors_per_cluster as usize) * (self.bytes_per_sector as usize),
                "read offset exceeds cluster size");

        let (sectors, start_offset) = { self.coords(cluster, offset) };
        let mut bytes_read = 0;
        let start_sector = sectors.start;
        for sector in sectors {
            let data = self.device.get(sector)?;
            bytes_read += if sector != start_sector {
                buf.write(&data)?
            } else {
                buf.write(&data[start_offset..])?
            }
        }
        Ok(bytes_read)
    }

    pub fn read_chain(
        &mut self,
        start: Cluster,
        buf: &mut Vec<u8>
    ) -> io::Result<usize> {
        let mut curr = start;
        let mut bytes_read = 0;
        loop {
            // parse the next entry ahead of time. This has the side-effect of
            // validating the current cluster is not a free or reserved cluster.
            let next = match self.fat_entry(curr)?.status() {
                Status::Data(cluster) => Ok(Some(cluster)),
                Status::Eoc(_) => Ok(None),
                Status::Reserved => panic!("trying to read reserved cluster"),
                Status::Free => panic!("trying to read free cluster"),
                Status::Bad => Err(io::Error::new(io::ErrorKind::InvalidData, "cluster contains bad sector(s)"))
            };
            bytes_read += self.read_cluster(curr, 0, buf)?;
            match next {
                Ok(Some(cluster)) => curr = cluster,
                Ok(None) => break,
                Err(err) => return Err(err),
            };
        };
        Ok(bytes_read)
    }


    pub fn fat_entry(&mut self, cluster: Cluster) -> io::Result<&FatEntry> {
        assert!(cluster.data_offset() < self.data_sectors, "cluster out of bounds");
        let cluster_fat_offset = cluster.id() * (size_of::<FatEntry>() as u32);
        let entry_sector = self.fat_start_sector + (cluster_fat_offset as u64) / (self.bytes_per_sector as u64);
        let entry_offset = (cluster_fat_offset % (self.bytes_per_sector as u32)) as usize;
        let fat_entry = &mut (self.device.get_mut(entry_sector)?[entry_offset]) as *mut u8 as *mut FatEntry;

        unsafe {
            Ok(&*fat_entry)
        }
    }

    fn root(&self, aref: &Shared<VFat>) -> Dir {
        Dir { fs: aref.clone(), start_cluster: self.root_dir_cluster, name: String::from(ROOT_NAME), metadata: ROOT_MD }
    }
}

impl<'a> FileSystem for &'a Shared<VFat> {
    type File = File;
    type Dir = Dir;
    type Entry = Entry;

    fn open<P: AsRef<Path>>(self, path: P) -> io::Result<Self::Entry> {
        let mut cwd = self.borrow_mut().root(self);
        let mut iter = path.as_ref().components().peekable();
        if iter.next() != Some(Component::RootDir) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not an absolute path"));
        }

        loop {
            let el = match iter.next() {
                Some(Component::Normal(x)) => x,
                Some(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "not an absolute path")),
                None => break,
            };
            match cwd.find(el) {
                Err(x) => {
                    match x.kind() {
                        io::ErrorKind::NotFound => {
                            if iter.peek().is_none() {
                                return Err(io::Error::new(io::ErrorKind::NotFound, "file not found"));
                            } else {
                                return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("directory does not exist: '{:?}'", el)));
                            }
                        },
                        _ => return Err(x),
                    }
                },
                Ok(Entry::Dir(d)) => cwd = d,
                Ok(Entry::File(f)) => {
                    if iter.peek().is_none() {
                        return Ok(Entry::File(f));
                    } else {
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("not a directory: '{:?}'", el)));
                    }
                }
            }
        }
        return Ok(Entry::Dir(cwd));
    }

    fn create_file<P: AsRef<Path>>(self, _path: P) -> io::Result<Self::File> {
        unimplemented!("read only file system")
    }

    fn create_dir<P>(self, _path: P, _parents: bool) -> io::Result<Self::Dir>
        where P: AsRef<Path>
    {
        unimplemented!("read only file system")
    }

    fn rename<P, Q>(self, _from: P, _to: Q) -> io::Result<()>
        where P: AsRef<Path>, Q: AsRef<Path>
    {
        unimplemented!("read only file system")
    }

    fn remove<P: AsRef<Path>>(self, _path: P, _children: bool) -> io::Result<()> {
        unimplemented!("read only file system")
    }
}
