use std::cmp::{min};
use std::io::{self, SeekFrom};

use traits;
use vfat::{VFat, Shared, Cluster, Metadata, Status};

#[derive(Debug)]
pub struct File {
    start_cluster: Cluster,
    fs: Shared<VFat>,
    pub name: String,
    pub metadata: Metadata,
    size: u32,
    pos: usize,
    curr: Cluster,
}

impl File {
    pub fn new(fs: Shared<VFat>, start_cluster: Cluster, name: String, metadata: Metadata, size: u32) -> Self {
        File {
            fs,
            start_cluster,
            name,
            metadata,
            size,
            pos: 0,
            curr: start_cluster
        }
    }
}

// FIXME: Implement `traits::File` (and its supertraits) for `File`.

impl traits::File for File {
    fn sync(&mut self) -> io::Result<()> {
        unimplemented!("File::sync")
    }

    fn size(&self) -> u64 {
        self.size as u64
    }
}

impl io::Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // println!("read: cluster={:?} name={:?}", self.start_cluster, self.name);
        let mut read = 0;
        let cluster_bytes = {
            let fs = self.fs.borrow();
            fs.bytes_per_sector as usize * fs.sectors_per_cluster as usize
        };
        if self.size == 0 {
            return Ok(0);
        }
        loop {
            if buf.len() - read == 0 {
                // println!("\nbuffer full. buf.len() = {:}", buf.len());
                break;
            }
            let cluster_offset = self.pos % cluster_bytes;
            let cluster_bytes_remaining = cluster_bytes - cluster_offset;
            let max_read = min(min(cluster_bytes_remaining, buf.len() - read), self.size as usize - self.pos as usize);
            let mut fs = self.fs.borrow_mut();
            let bytes_read = fs.read_cluster(self.curr, cluster_offset, &mut buf[read..(read + max_read)])?;
            read += bytes_read;
            self.pos += bytes_read;
            if self.pos == self.size as usize {
                // println!("\nend of file. file.size() = {:}", self.size);
                break;
            }
            if bytes_read == cluster_bytes_remaining {
                let entry = fs.fat_entry(self.curr)?;
                // println!("{:#?} bytes_read={:} cluster_bytes_remaining={:} cluster_bytes={:} max_read={:}", &self, bytes_read, cluster_bytes_remaining, cluster_bytes, max_read);
                match entry.status() {
                    Status::Data(cluster) => self.curr = cluster, 
                    Status::Eoc(_) => panic!("read past end of chain"),
                    Status::Reserved => panic!("read of reserved cluster"),
                    Status::Free => panic!("read of free cluster"),
                    Status::Bad => panic!("file contains bad sector(s)"),
                }
            }
        }
        // println!("read={:} pos={:}", read, self.pos);
        Ok(read)
    }
}

impl io::Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unimplemented!()
    }

    fn flush(&mut self) -> io::Result<()> {
        unimplemented!()
    }
}

impl io::Seek for File {
    /// Seek to offset `pos` in the file.
    ///
    /// A seek to the end of the file is allowed. A seek _beyond_ the end of the
    /// file returns an `InvalidInput` error.
    ///
    /// If the seek operation completes successfully, this method returns the
    /// new position from the start of the stream. That position can be used
    /// later with SeekFrom::Start.
    ///
    /// # Errors
    ///
    /// Seeking before the start of a file or beyond the end of the file results
    /// in an `InvalidInput` error.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        unimplemented!("File::seek()")
    }
}
