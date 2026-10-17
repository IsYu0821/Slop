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
pub mod process;
pub mod process_runtime;
pub mod sync;
pub mod task;
pub mod time;
pub mod vfs;
pub mod fd;
pub mod vm;
pub mod wait_queue;
pub mod usercopy;

#[repr(align(16))]
struct Stack([u8; 65536]);
static mut RING0_STACK: Stack = Stack([0; 65536]);
static mut DOUBLE_FAULT_STACK: Stack = Stack([0; 65536]);
static mut IRQ_STACK: Stack = Stack([0; 65536]);

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn kmain_bootinfo(info: *const boot::BootInfo) -> ! {
    console::init();
    if info.is_null() { println!("invalid null BootInfo"); arch::halt(); }
    let bi=unsafe { &*info };
    if !bi.validate() { println!("invalid BootInfo ABI"); arch::halt(); }
    let regions=unsafe { core::slice::from_raw_parts(bi.memory_map,bi.memory_map_len) };
    let pages=match memory::init(regions){Ok(n)=>n,Err(e)=>{println!("memory init failed: {}",e);arch::halt()}};
    let features=match unsafe { arch::x86_64::cpu::init() }{Ok(v)=>v,Err(e)=>{println!("CPU init failed: {}",e);arch::halt()}};
    let rsp0=unsafe { core::ptr::addr_of!(RING0_STACK.0) as u64 + 65536 };
    let ist=unsafe { core::ptr::addr_of!(DOUBLE_FAULT_STACK.0) as u64 + 65536 };
    let irq_stack=unsafe { core::ptr::addr_of!(IRQ_STACK.0) as u64 + 65536 };
    let irq_info=match unsafe { acpi::interrupt_info(bi.rsdp) }{Ok(v)=>v,Err(e)=>{println!("ACPI interrupt discovery failed: {}",e);arch::halt()}};
    unsafe { arch::x86_64::descriptors::init(rsp0,ist,irq_stack); arch::x86_64::idt::init(); arch::x86_64::apic::init(irq_info).unwrap_or_else(|e|{println!("APIC init failed: {}",e);arch::halt()}); arch::x86_64::syscall::init(rsp0); }
    println!("Slop/MNU 0.26.0: BootInfo accepted, {} free pages",pages);
    println!("interrupts: lapic={:#x} ioapic={:#x} timer-vector={:#x}",irq_info.lapic,irq_info.ioapic,arch::x86_64::apic::TIMER_VECTOR);
    let first_tick=match arch::x86_64::apic::self_test(){Ok(v)=>v,Err(e)=>{println!("APIC timer self-test failed: {}",e);arch::halt()}};
    println!("interrupts: local APIC timer and EOI passed tick={}",first_tick);
    println!("cpu: syscall={} nx={} 1g={} smep={} smap={} wp=1",features.syscall,features.nx,features.page_1g,features.smep,features.smap);
    driver::init(); task::init(rsp0); fd::init();
    if let Err(e)=task::scheduler_self_test(){println!("scheduler self-test failed: {}",e);arch::halt()}
    if bi.initrd_base==0 || bi.initrd_size==0 { println!("no embedded Linux ELF"); arch::halt(); }
    let initramfs=unsafe { core::slice::from_raw_parts(bi.initrd_base as *const u8,bi.initrd_size as usize) };
    let files=match vfs::mount_initramfs(initramfs){Ok(n)=>n,Err(e)=>{println!("initramfs mount failed errno={}",e);arch::halt()}};
    println!("vfs: initramfs mounted files={}",files);
    match vfs::image("/etc/motd"){Ok(v) if v==b"Slop/MNU initramfs\n"=>println!("vfs: path lookup and file read self-test passed"),_=>{println!("vfs self-test failed");arch::halt()}}
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
