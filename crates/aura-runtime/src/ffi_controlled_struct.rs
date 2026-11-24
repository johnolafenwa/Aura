//! libffi descriptors for layouts outside its naturally aligned structure
//! builder. The outer descriptor always gives the actual object extent.
use super::*;
use crate::c_layout::{CAbi, CPassing};

pub(super) fn descriptor(layout: &CStruct) -> Type {
    let abi = CAbi::host();
    let ty = if abi == Some(CAbi::SysV64)
        && layout.size() <= 16
        && layout.passing(CAbi::SysV64) == CPassing::Memory
    {
        // libffi's SysV classifier recursively classifies children. A child
        // wider than 32 bytes has the MEMORY class. This private marker forces
        // the class required by the psABI for a misaligned field. The call
        // engine copies only the parent descriptor's true size.
        let memory = Type::structure([Type::u8()]);
        set_extent(&memory, 33, 1);
        Type::structure([memory])
    } else {
        overlapping_struct_type(layout)
    };
    set_extent(&ty, layout.size(), layout.align());
    ty
}

fn set_extent(ty: &Type, size: usize, align: usize) {
    // SAFETY: callers construct a fresh owned struct descriptor for each
    // argument or result. libffi permits precomputed size and alignment.
    unsafe {
        (*ty.as_raw_ptr()).size = size;
        (*ty.as_raw_ptr()).alignment = align as u16;
    }
}
