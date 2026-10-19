#![no_std]
#![feature(naked_functions)]

pub mod arch;
pub mod boot;
pub mod acpi;
pub mod console;
pub mod driver;
pub mod elf;
pub mod errno;
pub mod exec;
pub mod exec_runtime;
pub mod linux_abi;
pub mod memory;
pub mod pipe;
pub mod socket;
pub mod net;
pub mod process;
pub mod signal;
pub mod sync;
pub mod task;
pub mod time;
pub mod vfs;
pub mod device;
pub mod ext2;
pub mod inotify;
pub mod fd;
pub mod flock;
pub mod posix_timer;
pub mod record_lock;
pub mod robust;
pub mod futex;
pub mod vm;
pub mod wait_queue;
pub mod usercopy;

#[repr(align(16))]
struct Stack([u8; 65536]);
static mut RING0_STACK: Stack = Stack([0; 65536]);
static mut DOUBLE_FAULT_STACK: Stack = Stack([0; 65536]);
static mut IRQ_STACK: Stack = Stack([0; 65536]);

static mut BOOT_PAGES:u64=0;
static mut BOOT_IRQ:Option<crate::acpi::InterruptInfo>=None;
static mut BOOT_INITRD:(u64,u64)=(0,0);

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn kmain_bootinfo(info: *const boot::BootInfo) -> ! {
    console::init();
    unsafe { core::arch::asm!("cli", options(nomem,nostack,preserves_flags)) };
    if info.is_null() { println!("invalid null BootInfo"); arch::halt(); }
    let bi=unsafe { &*info };
    if !bi.validate() { println!("invalid BootInfo ABI"); arch::halt(); }
    let regions=unsafe { core::slice::from_raw_parts(bi.memory_map,bi.memory_map_len) };
    let pages=match memory::init(regions){Ok(n)=>n,Err(e)=>{println!("memory init failed: {}",e);arch::halt()}};
    let features=match unsafe { arch::x86_64::cpu::init() }{Ok(v)=>v,Err(e)=>{println!("CPU init failed: {}",e);arch::halt()}};
    let irq_info=match unsafe { acpi::interrupt_info(bi.rsdp) }{Ok(v)=>v,Err(e)=>{println!("ACPI interrupt discovery failed: {}",e);arch::halt()}};
    unsafe { BOOT_PAGES=pages; BOOT_IRQ=Some(irq_info); BOOT_INITRD=(bi.initrd_base,bi.initrd_size); }
    if !features.page_1g { println!("kernel requires 1 GiB pages for the direct map"); arch::halt(); }
    /* Clone the firmware root into a kernel-owned PML4 (firmware tables are
       mapped read-only), install the direct map, then copy the image to
       KIMAGE_BASE and apply its PE base relocations so every absolute address
       in the kernel lives in the high half. The firmware's low mappings are
       dropped in kmain_high_continue; the low half then belongs to user
       space. */
    let fw=arch::x86_64::paging::PageTableRoot::current();
    let root=match fw.clone_bootstrap(&arch::x86_64::phys::EarlyPhys,&[]){Ok(r)=>r,Err(e)=>{println!("kernel CR3 clone failed errno={}",e);arch::halt()}};
    if let Err(e)=root.install_direct_map(&arch::x86_64::phys::EarlyPhys,2){println!("direct map install failed: {}",e);arch::halt()}
    let pages=(bi.image_size+4095)/4096;
    for i in 0..pages{
        let frame=match memory::alloc_frame(){Some(f)=>f,None=>{println!("kernel image allocation failed");arch::halt()}};
        if let Err(e)=root.map_page(&arch::x86_64::phys::EarlyPhys,arch::x86_64::phys::KIMAGE_BASE+i*4096,frame,arch::x86_64::paging::PRESENT|arch::x86_64::paging::WRITABLE){println!("kernel image mapping failed errno={}",e);arch::halt()}
    }
    unsafe { root.activate(); }
    unsafe { core::ptr::copy_nonoverlapping(bi.image_base as *const u8,arch::x86_64::phys::KIMAGE_BASE as *mut u8,bi.image_size as usize) };
    let delta=arch::x86_64::phys::KIMAGE_BASE as i64 - bi.image_base as i64;
    if let Err(e)=unsafe { boot::apply_relocations(arch::x86_64::phys::KIMAGE_BASE,delta) }{println!("kernel relocation failed: {}",e);arch::halt()}
    let rva=|va:u64|va-bi.image_base;
    let rsp=arch::x86_64::phys::KIMAGE_BASE+rva(unsafe { core::ptr::addr_of!(RING0_STACK.0) as u64 })+65536-8;
    let entry=arch::x86_64::phys::KIMAGE_BASE+rva(kmain_high_continue as usize as u64);
    unsafe { arch::x86_64::paging::enter_high(rsp,entry) }
}

/* Runs from the direct map with no low mapping left. */
#[inline(never)]
extern "sysv64" fn kmain_high_continue()->!{
    let root=arch::x86_64::paging::PageTableRoot::current();
    let base=((arch::x86_64::phys::PHYS_BASE>>39)&511)as usize;
    let kbase=((arch::x86_64::phys::KIMAGE_BASE>>39)&511)as usize;
    unsafe { root.keep_slots(&arch::x86_64::phys::Identity,&[base,base+1,kbase]); }
    let features=arch::x86_64::cpu::detect();
    let rsp0=unsafe { core::ptr::addr_of!(RING0_STACK.0) as u64 + 65536 };
    let ist=unsafe { core::ptr::addr_of!(DOUBLE_FAULT_STACK.0) as u64 + 65536 };
    let irq_stack=unsafe { core::ptr::addr_of!(IRQ_STACK.0) as u64 + 65536 };
    let irq_info=match unsafe { core::ptr::addr_of_mut!(BOOT_IRQ).read() }{Some(v)=>v,None=>{println!("boot hand-off data lost");arch::halt()}};
    unsafe { arch::x86_64::descriptors::init(rsp0,ist,irq_stack); arch::x86_64::idt::init(); arch::x86_64::apic::init(irq_info).unwrap_or_else(|e|{println!("APIC init failed: {}",e);arch::halt()}); arch::x86_64::syscall::init(rsp0); }
    let pages=unsafe { BOOT_PAGES };
    println!("Slop/MNU 0.27.0: high-half kernel, BootInfo accepted, {} free pages",pages);
    unsafe{arch::x86_64::port::outb(0x3f8,b'9')}
    println!("interrupts: lapic={:#x} ioapic={:#x} timer-vector={:#x}",irq_info.lapic,irq_info.ioapic,arch::x86_64::apic::TIMER_VECTOR);
    if let Some(secs)=arch::x86_64::rtc::epoch_seconds(){time::set_realtime_ns(secs.saturating_mul(1_000_000_000));println!("rtc: CLOCK_REALTIME seeded epoch={}",secs);}
    let first_tick=match arch::x86_64::apic::self_test(){Ok(v)=>v,Err(e)=>{println!("APIC timer self-test failed: {}",e);arch::halt()}};
    println!("interrupts: local APIC timer and EOI passed tick={}",first_tick);
    println!("cpu: syscall={} nx={} 1g={} smep={} smap={} wp=1",features.syscall,features.nx,features.page_1g,features.smep,features.smap);
    driver::init(); task::init(rsp0);
    if !fd::resource_self_test(){println!("fd resource-accounting self-test failed");arch::halt()}
    println!("resources: fd accounting self-test passed");
    if !vm::resource_self_test(){println!("mmap ledger self-test failed");arch::halt()}
    println!("resources: mmap ledger self-test passed");
    if !memory::refcount_self_test(){println!("memory refcount self-test failed");arch::halt()}
    println!("resources: memory refcount self-test passed");
    if let Err(e)=task::scheduler_self_test(){println!("scheduler self-test failed: {}",e);arch::halt()}
    let (initrd_base,initrd_size)=unsafe { BOOT_INITRD };
    if initrd_base==0 || initrd_size==0 { println!("no embedded Linux ELF"); arch::halt(); }
    let initramfs=unsafe { core::slice::from_raw_parts(arch::x86_64::phys::phys_to_virt(initrd_base) as *const u8,initrd_size as usize) };
    let files=match vfs::mount_initramfs(initramfs){Ok(n)=>n,Err(e)=>{println!("initramfs mount failed errno={}",e);arch::halt()}};
    println!("vfs: initramfs mounted files={}",files);
    match vfs::image("/etc/motd"){Ok(v) if v==b"Slop/MNU initramfs\n"=>println!("vfs: path lookup and file read self-test passed"),_=>{println!("vfs self-test failed");arch::halt()}}
    crate::device::init_devtmpfs();
    println!("device: devtmpfs ready");
    let _=crate::vfs::mkdir("/tmp",0o777);
    let _=crate::vfs::mkdir("/run",0o777);
    let _=crate::vfs::mkdir("/var",0o755);
    let _=crate::vfs::mkdir("/var/tmp",0o777);
    let _=crate::vfs::mkdir("/mnt",0o755);
    if !vfs::mount_self_test(){println!("vfs mount/namespace self-test failed");arch::halt()}
    println!("vfs: tmpfs mount and namespace self-test passed");
    let image=match vfs::image("/init"){Ok(v)=>v,Err(e)=>{println!("VFS /init lookup failed errno={}",e);arch::halt()}};
    let loaded=match unsafe { exec_runtime::load_static(image) }{Ok(v)=>v,Err(e)=>{println!("ELF commit failed: {}",e);arch::halt()}};
    println!("queueing Linux ELF rip={:#x} rsp={:#x} cr3={:#x}",loaded.entry,loaded.rsp,loaded.cr3);
    let user_pid=match task::spawn_user(loaded){Ok(pid)=>pid,Err(e)=>{println!("user task spawn failed errno={}",e);arch::halt()}};
    println!("task: user process runnable pid={}",user_pid);
    let status=match task::wait_child(user_pid){Ok(v)=>v,Err(e)=>{println!("wait for user task failed errno={}",e);arch::halt()}};
    println!("scheduler: user process reaped pid={} status={}",user_pid,status);
    arch::x86_64::qemu::exit(if status==0{0x10}else{0x11})
}

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn kmain(boot_magic: u64, boot_info: u64) -> ! {
    console::init();
    println!("Slop/MNU kernel 0.1");
    println!("boot magic={:#x} info={:#x}", boot_magic, boot_info);
    arch::x86_64::init();
    driver::init();
    task::init(0);
    println!("core initialized; entering scheduler");
    task::run()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    arch::halt()
}
