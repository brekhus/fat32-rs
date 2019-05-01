#![feature(decl_macro, conservative_impl_trait, dotdoteq_in_patterns)]
#![allow(safe_packed_borrows)]

#[cfg(not(target_endian="little"))]
compile_error!("only little endian platforms supported");

#[cfg(test)]
mod tests;
mod mbr;
mod util;

pub mod vfat;
pub mod traits;

pub use mbr::*;
