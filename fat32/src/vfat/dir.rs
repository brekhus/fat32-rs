use std::ffi::OsStr;
use std::borrow::{BorrowMut};
use std::io;
use std::vec::IntoIter;
use traits;
use util::VecExt;
use vfat::{VFat, Shared, File, Cluster, Entry, Status};
use vfat::{Metadata, Attributes, Timestamp,  Date};

#[derive(Debug)]
pub struct Dir {
    pub fs: Shared<VFat>,
    pub start_cluster: Cluster,
    pub name: String,
    pub metadata: Metadata
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct VFatRegularDirEntry {
    name: [u8; 8],
    ext: [u8; 3],
    attribs: u8,
    _reserved: u8,
    creation_decisecs: u8,
    created: Timestamp,
    accessed: Date,
    hi_cluster_part: u16,
    modified: Timestamp,
    lo_cluster_part: u16,
    size: u32
}

enum RegularSeq {
    Deleted,
    EndOfDirectory,
    Valid
}

impl VFatRegularDirEntry {
    fn seq(&self) -> RegularSeq {
        match self.name[0] {
            0xE5 => RegularSeq::Deleted,
            0 => RegularSeq::EndOfDirectory,
            _ => RegularSeq::Valid,
        }
    }

    fn checksum(&self) -> u8 {
        let mut sum = 0;
        for part in &[self.name.as_ref(), self.ext.as_ref()] {
            for chr in *part {
                sum = (((sum & 1) << 7) as u8)
                    .wrapping_add((sum >> 1) as u8)
                    .wrapping_add(*chr);
            }
        }
        sum
    }

    fn name(&self, lfn: LfnEnt) -> String {
        if let LfnEnt::End(checksum, name, len) = lfn {
            if checksum == self.checksum() {
                let mut i = 0;
                for &chr in &name[0..((len + 1) as usize)] {
                    if chr == 0 || chr == 0xFF {
                        break;
                    }
                    i += 1;
                }

                let name = String::from_utf16(&name[0..i]).expect("invalid long name");
                return name;
            }
        }

        let mut name = Vec::with_capacity(12);
        let sep = if self.ext[0] != 0x20  {
            [0x2E /* . */]
        } else {
            [0x20] // don't know how to abstract over arrays of differing lengths
        };

        for &part in &[self.name.as_ref(), sep.as_ref(), self.ext.as_ref()] {
            name.extend(part.iter().take_while(|&&x| x != 0 && x != 0x20));
        }
        String::from_utf8(name).expect("invalid dos name")
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            attribs: Attributes(self.attribs),
            created: self.created,
            accessed: self.accessed,
            modified: self.modified
        }
    }

    fn start_cluster(&self) -> Cluster {
        let id = (self.hi_cluster_part as u32) << 16 | (self.lo_cluster_part as u32);
        Cluster::from(id)
    }

    fn into_entry(self, lfn_ent: LfnEnt, fs: Shared<VFat>) -> Entry {
        let name = self.name(lfn_ent);
        let metadata = self.metadata();
        let start_cluster = self.start_cluster();
        if self.attribs & 0x10 == 0x10 { // its a dir
            Entry::Dir(Dir { fs, start_cluster, name, metadata, })
        } else {
            Entry::File(File::new(fs, start_cluster, name, metadata, self.size))
        }
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct VFatLfnDirEntry {
    sequence_number: u8,
    name_part_1: [u16; 5],
    attribs: u8,
    dtype: u8,
    checksum: u8,
    name_part_2: [u16; 6],
    reserved_: u16,
    name_part_3: [u16; 2],
}

#[derive(Debug)]
enum LfnSeq {
    Deleted,
    EndOfDirectory,
    Seq(u8, bool, bool),
}

impl VFatLfnDirEntry {
    fn seq(&self) -> LfnSeq {
        if self.sequence_number == 0 {
            LfnSeq::EndOfDirectory 
        } else if self.sequence_number == 0xE5 {
            LfnSeq::Deleted
        } else {
            let first = (0x20 & self.sequence_number) == 0;
            let last = (0x40 & self.sequence_number) == 0x40;
            LfnSeq::Seq(self.sequence_number & 0b11111, first, last)
        }
    }

    fn extend_name(&self, mut name: Vec<u16>) -> Vec<u16> {
        let mut i = 0;
        let base = ((self.sequence_number & 0b11111) - 1) as usize * 13;
        for &part in &[self.name_part_1.as_ref(), self.name_part_2.as_ref(), self.name_part_3.as_ref()] {
            for &chr in part {
                name[base + i] = chr;
                i += 1;
            }
        }
        name
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct VFatUnknownDirEntry {
    seq: u8,
    unknown_1: [u8; 10],
    attribs: u8,
    dtype: u8,
    unknown_2: [u8; 13],
    clust_num: u16,
    unknown_3: [u8; 4]
}

#[allow(dead_code)]
pub union VFatDirEntry {
    unknown: VFatUnknownDirEntry,
    regular: VFatRegularDirEntry,
    long_filename: VFatLfnDirEntry,
}

pub enum DirEntry<'a> {
    Regular(&'a VFatRegularDirEntry),
    Lfn(&'a VFatLfnDirEntry),
}

impl<'a> From<&'a VFatUnknownDirEntry> for DirEntry<'a> {
    fn from(dirent: &VFatUnknownDirEntry) -> DirEntry {
        if dirent.attribs == 0x0f && dirent.dtype == 0 && dirent.clust_num == 0 {
            DirEntry::Lfn(unsafe { &*(dirent as *const VFatUnknownDirEntry as *const VFatLfnDirEntry) })
        } else {
            DirEntry::Regular(unsafe { &*(dirent as *const VFatUnknownDirEntry as *const VFatRegularDirEntry) })
        }
    }
}

impl Dir {
    /// Finds the entry named `name` in `self` and returns it. Comparison is
    /// case-insensitive.
    ///
    /// # Errors
    ///
    /// If no entry with name `name` exists in `self`, an error of `NotFound` is
    /// returned.
    ///
    /// If `name` contains invalid UTF-8 characters, an error of `InvalidInput`
    /// is returned.
    pub fn find<P: AsRef<OsStr>>(&self, name: P) -> io::Result<Entry> {
        use traits::{Dir, Entry};
        if let Some(name_utf8) = name.as_ref().to_str() {
            match self.entries()?.find(|ref x| x.name().eq_ignore_ascii_case(name_utf8)) {
                Some(entry) => Ok(entry),
                None => Err(io::Error::new(io::ErrorKind::NotFound, "file not found")),
            }
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid utf-8 in name"))
        }
    }
}

pub struct DirIter {
    next: Option<Cluster>,
    fs: Shared<VFat>,
    curr_iter: Option<IntoIter<VFatUnknownDirEntry>>
}

#[derive(Debug)]
enum LfnEnt {
    None,
    Pos(u8, u8, Vec<u16>, u16),
    End(u8, Vec<u16>, u16)
}

impl LfnEnt {
    fn next(self, seq: LfnSeq, lfn: &VFatLfnDirEntry) -> LfnEnt {
        if let LfnSeq::Seq(pos, _first, last) = seq {
            if last {
                let mut name = match self {
                    LfnEnt::Pos(_, _, mut n, _) => n,
                    LfnEnt::End(_, mut n, _) => n,
                    LfnEnt::None => vec![0u16; 260],
                };

                if pos != 1 {
                    LfnEnt::Pos(pos, lfn.checksum, lfn.extend_name(name), (pos as u16) * 13)
                } else {
                    LfnEnt::End(lfn.checksum, lfn.extend_name(name), (pos as u16) * 13)
                }
            } else {
                if let LfnEnt::Pos(curr_pos, curr_checksum, name, length) = self {
                    if curr_pos - 1 == pos && curr_checksum == lfn.checksum {
                        if pos != 1 {
                            LfnEnt::Pos(pos, lfn.checksum, lfn.extend_name(name), length)
                        } else {
                            LfnEnt::End(lfn.checksum, lfn.extend_name(name), length)
                        }
                    } else {
                        LfnEnt::Pos(curr_pos, curr_checksum, name, length)
                    }
                } else {
                    self
                }
            }
        } else {
            panic!("unexpected LfnSeq variant")
        }
    }
}

impl Iterator for DirIter {
    type Item = Entry;

    fn next(&mut self) -> Option<Self::Item> {
        let mut lfn_ent = LfnEnt::None;
        loop {
            if self.curr_iter.is_some() {
                let iter = &mut *self.curr_iter.as_mut().unwrap();
                for entry in iter {
                    let dirent = DirEntry::from(&entry);
                    match dirent {
                        DirEntry::Regular(ref r) => {
                            match r.seq() {
                                RegularSeq::Deleted => continue,
                                RegularSeq::EndOfDirectory => return None,
                                RegularSeq::Valid => return Some(r.into_entry(lfn_ent, self.fs.clone())),
                            };
                        },
                        DirEntry::Lfn(ref lfn) => {
                            let seq = lfn.seq();
                            match seq {
                                LfnSeq::Deleted => continue,
                                LfnSeq::EndOfDirectory => return None,
                                LfnSeq::Seq(_, _, _) => { lfn_ent = lfn_ent.next(seq, lfn) },
                            };
                        }
                    };
                }
            } 

            if let Some(cluster) = self.next {
                let mut fs = self.fs.borrow_mut();
                let mut buf = Vec::with_capacity(fs.bytes_per_sector as usize * fs.sectors_per_cluster as usize);
                unsafe {
                    buf.set_len(fs.bytes_per_sector as usize * fs.sectors_per_cluster as usize);
                }

                let bytes_read = fs.borrow_mut().read_cluster(cluster, 0, &mut buf).expect("read of directory failed");
                assert_eq!(bytes_read, buf.capacity());
                let dirents : Vec<VFatUnknownDirEntry> = unsafe { buf.cast() };
                self.curr_iter = Some(dirents.into_iter());
                self.next = match fs.fat_entry(cluster).expect("directory cluster lookup failed").status() {
                    Status::Data(cluster) => Some(cluster),
                    Status::Eoc(_) => None,
                    Status::Reserved => panic!("directory chain has a reserved cluster"),
                    Status::Free => panic!("directory chain has a free cluster"),
                    Status::Bad => panic!("directory chain has bad sector(s)"),
                };
            } else {
                panic!("read last cluster before end of directory");
            }
        }
    }
}

impl traits::Dir for Dir {
    type Entry = Entry;
    type Iter = DirIter;
    fn entries(&self)-> io::Result<Self::Iter> {
        Ok(DirIter { 
            fs: self.fs.clone(),
            next: Some(self.start_cluster),
            curr_iter: None,
        })
    }
}