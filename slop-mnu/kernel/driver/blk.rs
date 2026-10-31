//! Block backend facade: virtio-blk first (legacy QEMU profile), then
//! native NVMe (generic PC profile), then AHCI/SATA (generic PC alt).
//! Callers (ext2/ext4/device) use 512B sectors and never name a backend;
//! the first ready device wins so the QEMU regression profile behaves
//! exactly as before.
use crate::errno;

pub fn ready() -> bool {
    crate::driver::virtio_blk::ready() || crate::driver::nvme::ready() || crate::driver::ahci::ready()
}

pub fn capacity() -> u64 {
    if crate::driver::virtio_blk::ready() {
        return crate::driver::virtio_blk::capacity();
    }
    if crate::driver::nvme::ready() {
        return crate::driver::nvme::capacity();
    }
    crate::driver::ahci::capacity()
}

pub fn read_sector(sector: u64, dst: &mut [u8]) -> Result<(), i64> {
    if crate::driver::virtio_blk::ready() {
        return crate::driver::virtio_blk::read_sector(sector, dst);
    }
    if crate::driver::nvme::ready() {
        return crate::driver::nvme::read_sector(sector, dst);
    }
    if crate::driver::ahci::ready() {
        return crate::driver::ahci::read_sector(sector, dst);
    }
    Err(errno::ENODEV)
}

pub fn write_sector(sector: u64, src: &[u8]) -> Result<(), i64> {
    if crate::driver::virtio_blk::ready() {
        return crate::driver::virtio_blk::write_sector(sector, src);
    }
    if crate::driver::nvme::ready() {
        return crate::driver::nvme::write_sector(sector, src);
    }
    if crate::driver::ahci::ready() {
        return crate::driver::ahci::write_sector(sector, src);
    }
    Err(errno::ENODEV)
}
