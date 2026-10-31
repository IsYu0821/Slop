//! Network backend facade: virtio-net first (legacy QEMU profile), then
//! a generic PCI NIC (e1000, generic PC profile). Callers (net stack)
//! use Ethernet frames and never name a backend; the first ready device
//! wins so the QEMU regression profile behaves exactly as before.
pub fn ready() -> bool {
    crate::driver::virtio_net::ready() || crate::driver::e1000::ready()
}
pub fn mac() -> [u8; 6] {
    if crate::driver::virtio_net::ready() {
        return crate::driver::virtio_net::mac();
    }
    crate::driver::e1000::mac()
}
pub fn poll_frame(out: &mut [u8]) -> Option<usize> {
    if crate::driver::virtio_net::ready() {
        return crate::driver::virtio_net::poll_frame(out);
    }
    if crate::driver::e1000::ready() {
        return crate::driver::e1000::poll_frame(out);
    }
    None
}
pub fn send_frame(frame: &[u8]) -> Result<(), i64> {
    if crate::driver::virtio_net::ready() {
        return crate::driver::virtio_net::send_frame(frame);
    }
    if crate::driver::e1000::ready() {
        return crate::driver::e1000::send_frame(frame);
    }
    Err(crate::errno::ENODEV)
}
