use std::fmt;

use vfat::*;

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Hash)]
pub struct Cluster(pub u32);

impl From<u32> for Cluster {
    fn from(raw_num: u32) -> Cluster {
        Cluster(raw_num & !(0xF << 28))
    }

}

impl Cluster {

    pub fn offset(&self) -> u32 {
        self.0 - 1
    }

    pub fn has_next(&self) -> bool {
        match self.0 {
            0x2..=0xFFFFFEF => true,
            0xFFFFFF8..=0xFFFFFFF => false,
            _=> panic!("invalid cluster address: cluster=0x?{:03x}", self.0),
        }
    }
}

impl fmt::Debug for Cluster {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Cluster(0x?{:07x})", self.0)
    }
}


// TODO: Implement any useful helper methods on `Cluster`.
