use std::ffi::OsStr;
use std::char::decode_utf16;
use std::borrow::{Cow, BorrowMut};
use std::io;
use std::vec::IntoIter;
use traits;
use util::VecExt;
use vfat::{VFat, Shared, File, Cluster, Entry, Status};
use vfat::{Metadata, Attributes, Timestamp, Time, Date};

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
        if let LfnEnt::End(checksum, name) = lfn {
            if checksum == self.checksum() {
                return String::from_utf16(&name).expect("invalid long name");
            }
        }
        let mut name = Vec::with_capacity(12);
        // directories cant't have 8.3 extensions
        if self.attribs & 0x10 == 0 {
            for &part in &[self.name.as_ref(), [0x2E /* . */].as_ref(), self.ext.as_ref()] {
                name.extend(part.iter().take_while(|&&x| x != 0 && x != 0x20));
            }
        } else {
            name.extend(self.name.iter().take_while(|&&x| x != 0 && x != 0x20));
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
        let num = (self.hi_cluster_part as u32) << 16 | (self.lo_cluster_part as u32);
        Cluster::from(num)
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
            // println!("seq={:02x} first={:?} last={:?}", self.sequence_number, first, last);
            LfnSeq::Seq(self.sequence_number & 0b11111, first, last)
        }
    }

    fn extend_name(&self, mut name: Vec<u16>) -> Vec<u16> {
        for &part in &[self.name_part_1.as_ref(), self.name_part_2.as_ref(), self.name_part_3.as_ref()] {
            for chr in part {
                if *chr == 0x00 || *chr == 0xFF {
                    return name;
                }
                name.push(*chr);
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
    Pos(u8, u8, Vec<u16>),
    End(u8, Vec<u16>)
}

impl LfnEnt {
    fn next(self, seq: LfnSeq, lfn: &VFatLfnDirEntry) -> LfnEnt {
        if let LfnSeq::Seq(pos, first, last) = seq {
            if last {
                let mut name = match self {
                    LfnEnt::Pos(_, _, mut n) => { n.clear(); n },
                    LfnEnt::End(_, mut n) => { n.clear(); n },
                    LfnEnt::None => { Vec::with_capacity(255) },
                };

                if pos != 1 {
                    LfnEnt::Pos(pos, lfn.checksum, lfn.extend_name(name))
                } else {
                    LfnEnt::End(lfn.checksum, lfn.extend_name(name))
                }
            } else {
                if let LfnEnt::Pos(curr_pos, curr_checksum, name) = self {
                    if curr_pos - 1 == pos && curr_checksum == lfn.checksum {
                        if pos != 1 {
                            LfnEnt::Pos(pos, lfn.checksum, lfn.extend_name(name))
                        } else {
                            LfnEnt::End(lfn.checksum, lfn.extend_name(name))
                        }
                    } else {
                        LfnEnt::Pos(curr_pos, curr_checksum, name)
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
            // println!("{:?}", lfn_ent);
            if self.curr_iter.is_some() {
                let iter = &mut *self.curr_iter.as_mut().unwrap();
                for entry in iter {
                    let dirent = DirEntry::from(&entry);
                    match dirent {
                        DirEntry::Regular(ref r) => {
                            match r.seq() {
                                RegularSeq::Deleted => continue,
                                RegularSeq::EndOfDirectory =>  {
                                    // println!("end of directory");
                                    return None
                                },
                                RegularSeq::Valid => {
                                    let name = r.name(lfn_ent);
                                    let metadata = r.metadata();
                                    if r.attribs & 0x10 != 0 {
                                        println!("dir name={:} md={:?}", name, metadata);
                                        return Some(Entry::Dir(Dir { 
                                            fs: self.fs.clone(),
                                            start_cluster: r.start_cluster(), 
                                            name,
                                            metadata,
                                        }));
                                    } else {
                                        println!("file name={:} md={:?}", name, metadata);
                                        return Some(Entry::File(File::new(
                                            self.fs.clone(),
                                            r.start_cluster(),
                                            name,
                                            metadata,
                                            r.size)));
                                    }
                                }
                            };
                        },
                        DirEntry::Lfn(ref lfn) => {
                            let seq = lfn.seq();
                            // println!("{:?}", seq);
                            match seq {
                                LfnSeq::Deleted => continue,
                                LfnSeq::EndOfDirectory => return None,
                                LfnSeq::Seq(_, _, _) => { 
                                    lfn_ent = lfn_ent.next(seq, lfn); 
                                    // println!("{:?}", lfn_ent);
                                },
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
                let mut i = 0;
                for &byte in &buf {
                    print!("{:02x}", byte);
                    i += 1;
                    if i % 16 == 0 {
                        println!();
                    } else if i % 2 == 0 {
                        print!(" ");
                    }
                }
                println!();
                let dirents : Vec<VFatUnknownDirEntry> = unsafe { buf.cast() };
                for (i, &dirent) in dirents.iter().enumerate() {
                    println!("dirents[{:02}] = Dirent(seq={:02x}, attribs={:02x} dtype={:02x} clust_num={:02x})", i, dirent.seq, dirent.attribs, dirent.dtype, dirent.clust_num);
                }
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
        println!("directory='{:}' entries", self.name);
        Ok(DirIter { 
            fs: self.fs.clone(),
            next: Some(self.start_cluster),
            curr_iter: None,
        })
    }
}