pub mod syscall;
pub mod msr;
pub mod descriptors;
pub mod idt;
pub mod paging;
pub mod phys;
pub mod user;
pub mod qemu;
pub mod cpu;
pub mod apic;
pub mod port;

pub fn init() {
    // Milestone 2: install GDT/IDT, APIC, timer, page tables and SYSCALL MSRs.
    crate::println!("x86_64: architecture seam ready");
}
