//! Symmetric multiprocessing bring-up: parked APs with per-CPU state.
//!
//! MS82 scope: enumerate Local APICs from the MADT, start every enabled
//! application processor with INIT-SIPI-SIPI, give each its own kernel
//! stack, GDT, TSS and IDT mapping plus a per-CPU data slot, and park
//! it. The BSP verifies every AP reached 64-bit kernel code on its own
//! stack by checking a per-CPU attestation, then continues with the
//! available CPU set when a deadline expires instead of hanging.
//! Scheduling on APs, IPIs and TLB shootdown belong to later milestones.
//! MS93: after attestation each AP enters the shared run-queue scheduler
//! (task::ap_enter) instead of halting; see kernel/task.rs.
use core::sync::atomic::{AtomicBool,AtomicU32,AtomicU64,Ordering};

use crate::arch::x86_64::paging::PhysAccess;

/// Upper bound for tracked CPUs; keeps every per-CPU table static.
pub const MAX_CPUS: usize = 64;
/// Physical address of the AP trampoline (identity-mapped, one page).
pub const TRAMPOLINE_ADDR: u64 = 0x7000;
/// SIPI vector naming the trampoline page.
pub const TRAMPOLINE_VECTOR: u8 = (TRAMPOLINE_ADDR >> 12) as u8;
/// Per-CPU stack in bytes: one frame is plenty for bring-up plus the
/// parked loop (no interrupts or syscalls run on APs yet). MS83 gives
/// APs full scheduler stacks.
pub const AP_STACK_SIZE: usize = 4096;

/// Per-CPU states, also the attestation protocol.
pub const CPU_ABSENT: u32 = 0;
pub const CPU_PRESENT: u32 = 1;
pub const CPU_STARTED: u32 = 2;
pub const CPU_READY: u32 = 3;

/// Attestation magic written by each CPU that completes bring-up.
const ATTEST_MAGIC: u64 = 0x5c0ffee00ff17811;

static CPU_STATE: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(CPU_ABSENT) }; MAX_CPUS];
static CPU_STACK_TOP: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static CPU_PROBE_RSP: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static CPU_MAGIC: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static CPU_COUNTER: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static mut CPU_GDT: [[u64; 7]; MAX_CPUS] = [[0; 7]; MAX_CPUS];
static mut CPU_TSS: [crate::arch::x86_64::descriptors::Tss; MAX_CPUS] =
    [const { crate::arch::x86_64::descriptors::Tss::new() }; MAX_CPUS];

/// Flat tables the trampoline indexes by CPU number.
static mut TRAMP_IDS: [u32; MAX_CPUS] = [0xFFFF_FFFF; MAX_CPUS];
static mut TRAMP_STACKS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Number of MADT entries accepted (enabled or not).
static CPU_COUNT: AtomicU32 = AtomicU32::new(0);
/// Number of CPUs that completed bring-up, BSP included.
static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);
/// BSP's slot index.
static BSP_SLOT: AtomicU32 = AtomicU32::new(0);
/// Set by the BSP once the scheduler is ready to accept APs.
static SCHED_READY: AtomicBool = AtomicBool::new(false);

pub fn sched_ready() -> bool {
    SCHED_READY.load(Ordering::Acquire)
}

pub fn set_sched_ready(v: bool) {
    SCHED_READY.store(v, Ordering::Release)
}

pub fn bsp_slot() -> usize {
    BSP_SLOT.load(Ordering::Relaxed) as usize
}

pub fn online_count() -> u32 {
    CPU_ONLINE.load(Ordering::Relaxed)
}

pub fn apic_id_of_slot(slot: usize) -> u32 {
    if slot >= MAX_CPUS {
        return 0xFFFF_FFFF;
    }
    unsafe { core::ptr::read_volatile(&raw const TRAMP_IDS[slot]) }
}

/// Writes an AP's TSS RSP0 (used on task switch and idle entry). The BSP
/// uses BOOT_TSS (see descriptors::set_rsp0); APs use their CPU_TSS slot.
pub fn set_tss_rsp0(slot: usize, v: u64) {
    if slot >= MAX_CPUS {
        return;
    }
    unsafe {
        (*(&raw mut CPU_TSS[slot])).rsp[0] = v;
    }
}
pub fn tss_diag(slot: usize) -> (u64, u64, u64) {
    if slot >= MAX_CPUS {
        return (0, 0, 0);
    }
    unsafe {
        let tss = &raw const CPU_TSS[slot];
        ((*tss).rsp[0], (*tss).ist[0], (*tss).ist[1])
    }
}
/// DIAG: returns (tss_addr, gdt_desc_base, gdt_limit) for an AP slot.
pub fn gdt_diag(slot: usize) -> (u64, u64, u64) {
    if slot >= MAX_CPUS {
        return (0, 0, 0);
    }
    unsafe {
        let tss_addr = &raw const CPU_TSS[slot] as u64;
        let g = &raw const CPU_GDT[slot];
        let lo = core::ptr::read_volatile(g.cast::<[u64; 7]>().add(0).cast::<u64>().add(5));
        let hi = core::ptr::read_volatile(g.cast::<[u64; 7]>().add(0).cast::<u64>().add(6));
        let base = ((lo >> 16) & 0xffffff) | ((lo >> 32) & 0xff000000) | (hi << 32);
        let limit = (lo & 0xffff) | ((lo >> 32) & 0xf0000);
        (tss_addr, base, limit)
    }
}
/// Points an AP's TSS at its scheduler idle stack (called before the AP
/// enables interrupts in the scheduler). The GDT/TR bases are unchanged.
pub fn ap_sched_stack(slot: usize, idle_top: u64) {
    if slot >= MAX_CPUS {
        return;
    }
    unsafe {
        let tss = &raw mut CPU_TSS[slot];
        (*tss).rsp[0] = idle_top;
        (*tss).ist[0] = idle_top.wrapping_sub(8192);
        (*tss).ist[1] = idle_top.wrapping_sub(16384);
    }
}

/// How long the BSP waits for one AP to report ready. Bounded so a dead
/// AP can never hang the boot; completion is observed through the
/// per-CPU state, never inferred from elapsed time alone.
fn wait_cycles() -> u64 {
    let hz = crate::time::tsc_hz();
    if hz == 0 {
        return 50_000_000;
    }
    hz.saturating_mul(2)
}

/// Microsecond delay from the time-stamp counter for the INIT/SIPI
/// protocol timings.
fn udelay(us: u64) {
    let hz = crate::time::tsc_hz();
    if hz == 0 {
        for _ in 0..us.saturating_mul(5000) {
            core::hint::spin_loop();
        }
        return;
    }
    let start = crate::arch::x86_64::tsc::rdtsc();
    let delta = hz.saturating_mul(us).saturating_add(999_999) / 1_000_000;
    let deadline = start.saturating_add(delta.max(1));
    while crate::arch::x86_64::tsc::rdtsc() < deadline {
        core::hint::spin_loop();
    }
}

unsafe extern "C" {
    static trampoline_start: u8;
    static trampoline_end: u8;
}

fn trampoline_size() -> usize {
    unsafe { (&raw const trampoline_end as usize) - (&raw const trampoline_start as usize) }
}

/// Moves a page-table page currently living at physical `pa` to a fresh
/// frame, rewriting every table reference to it. The SIPI trampoline must
/// sit at the fixed low address 0x7000, but our own early tables may
/// occupy that frame; zeroing it in place would silently unmap live
/// kernel text (the first call into the wiped range then takes a fetch
/// fault through a stale TLB gap).
fn relocate_table_at(pa: u64) -> Result<(), &'static str> {
    use crate::arch::x86_64::paging::{ADDR, PRESENT, PageTableRoot};
    use crate::arch::x86_64::phys::Identity;
    let root = PageTableRoot::current();
    let acc = Identity;
    let pml4t = unsafe { acc.table(root.pml4) };
    for i in 0..512 {
        let e = pml4t[i];
        if e & PRESENT == 0 || e & (1 << 7) != 0 {
            continue;
        }
        if e & ADDR == pa {
            move_table(&root, &acc, &mut pml4t[i], pa)?;
            continue;
        }
        let pdpt = unsafe { acc.table(e & ADDR) };
        for j in 0..512 {
            let e2 = pdpt[j];
            if e2 & PRESENT == 0 || e2 & (1 << 7) != 0 {
                continue;
            }
            if e2 & ADDR == pa {
                move_table(&root, &acc, &mut pdpt[j], pa)?;
                continue;
            }
            let pd = unsafe { acc.table(e2 & ADDR) };
            for k in 0..512 {
                let e3 = pd[k];
                if e3 & PRESENT == 0 || e3 & (1 << 7) != 0 {
                    continue;
                }
                if e3 & ADDR == pa {
                    move_table(&root, &acc, &mut pd[k], pa)?;
                }
            }
        }
    }
    Ok(())
}

/// Copies the table at `pa` to a fresh frame and repoints `entry` at the
/// copy, then reloads CR3 so no stale TLB entry can reference the old
/// frame. The old and new contents are identical, so the reload is safe
/// even if a timer tick fires midway.
fn move_table(
    root: &crate::arch::x86_64::paging::PageTableRoot,
    acc: &crate::arch::x86_64::phys::Identity,
    entry: &mut u64,
    pa: u64,
) -> Result<(), &'static str> {
    use crate::arch::x86_64::paging::ADDR;
    use crate::arch::x86_64::phys::phys_to_virt;
    let old = *entry;
    let new = crate::memory::alloc_frame().ok_or("no frame to relocate table")?;
    if new == pa {
        return Err("allocator returned the trampoline frame");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            phys_to_virt(pa) as *const u8,
            phys_to_virt(new) as *mut u8,
            4096,
        );
    }
    *entry = (new & ADDR) | (old & !ADDR);
    unsafe { root.activate() };
    crate::println!("smp: relocated table {:#x} -> {:#x}", pa, new);
    Ok(())
}

/// Identity-maps one page for the trampoline and copies the blob there.
/// Returns the physical address on success; the mapping stays (the APs
/// keep executing from it while parked, and removing it would need the
/// TLB shootdown that arrives with a later milestone).
fn install_trampoline() -> Result<u64, &'static str> {
    let size = trampoline_size();
    if size == 0 || size > 0x1000 {
        return Err("trampoline image is empty or too large");
    }
    // The AP starts with paging OFF: after SIPI it fetches PHYSICAL
    // (vector << 12). The blob hardcodes base 0x7000 everywhere, so the
    // page must physically sit at 0x7000 (the conventional SMP spot,
    // free conventional memory under OVMF/QEMU). An allocated high frame
    // would only be visible through paging the AP does not have yet.
    // Our own early page tables may live in that frame; move any table
    // out of the way first (zeroing it in place unmaps live text).
    relocate_table_at(TRAMPOLINE_ADDR)?;
    let frame = TRAMPOLINE_ADDR;
    let root = crate::arch::x86_64::paging::PageTableRoot::current();
    root.map_page(
        &crate::arch::x86_64::phys::Identity,
        TRAMPOLINE_ADDR,
        frame,
        crate::arch::x86_64::paging::PRESENT | crate::arch::x86_64::paging::WRITABLE,
    )
    .map_err(|_| "trampoline identity map failed")?;
    unsafe {
        let dst = crate::arch::x86_64::phys::phys_to_virt(frame) as *mut u8;
        core::ptr::write_bytes(dst, 0, 0x1000);
        core::ptr::copy_nonoverlapping(
            &raw const trampoline_start as *const u8,
            dst,
            size,
        );
        core::sync::atomic::fence(Ordering::SeqCst);
    }
    Ok(frame)
}

/// Runs on every application processor once the trampoline jumps high.
/// Establishes this CPU's GDT/TSS/IDT view and APIC, attests, then waits
/// for the scheduler and enters it (never returns once scheduled).
#[unsafe(no_mangle)]
pub extern "sysv64" fn ap_main(apic_id: u32, slot: usize) -> ! {
    if slot >= MAX_CPUS {
        unsafe { crate::arch::halt() }
    }
    CPU_STATE[slot].store(CPU_STARTED, Ordering::Release);
    unsafe {
        crate::arch::x86_64::msr::wrmsr(
            crate::arch::x86_64::msr::IA32_KERNEL_GS_BASE,
            per_cpu_ptr(slot) as u64,
        );
        crate::arch::x86_64::msr::wrmsr(
            crate::arch::x86_64::msr::IA32_GS_BASE,
            per_cpu_ptr(slot) as u64,
        );
        let tss = &raw mut CPU_TSS[slot];
        let top = CPU_STACK_TOP[slot].load(Ordering::Relaxed);
        (*tss).rsp[0] = top;
        (*tss).ist[0] = top.wrapping_sub(8192);
        (*tss).ist[1] = top.wrapping_sub(16384);
        crate::arch::x86_64::descriptors::fill_gdt(
            &raw mut CPU_GDT[slot] as *mut [u64; 7],
            tss as u64,
        );
        crate::arch::x86_64::descriptors::load_gdt(
            &raw const CPU_GDT[slot] as *const [u64; 7] as u64,
        );
        crate::arch::x86_64::descriptors::load_tr();
        /* The trampoline entered long mode with its own CS; reload the
           kernel CS now that the kernel GDT is live, otherwise the first
           interrupt return reloads a stale selector and #GPs. */
        crate::arch::x86_64::descriptors::reload_cs();
        crate::arch::x86_64::idt::reload();
        crate::arch::x86_64::apic::ap_enable();
    }
    // Attest: the stack pointer must lie inside this CPU's own stack.
    let rsp: u64;
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nostack, preserves_flags)) };
    let top = CPU_STACK_TOP[slot].load(Ordering::Relaxed);
    let in_range = top != 0 && rsp <= top && rsp > top.wrapping_sub(AP_STACK_SIZE as u64);
    CPU_PROBE_RSP[slot].store(if in_range { rsp } else { 0 }, Ordering::Relaxed);
    CPU_MAGIC[slot].store(ATTEST_MAGIC, Ordering::Relaxed);
    CPU_COUNTER[slot].fetch_add(1, Ordering::Relaxed);
    let _ = apic_id;
    // Release: the BSP's waiter observes READY only after every field above.
    CPU_STATE[slot].store(CPU_READY, Ordering::Release);
    // Wait for scheduler init, then join the shared run queue (never returns).
    while !SCHED_READY.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    crate::task::ap_enter(slot)
}

/// Waits for one AP's attestation with a TSC deadline. Returns true only
/// when the AP itself reported ready; a timeout leaves the slot parked.
fn wait_ready(slot: usize, deadline: u64) -> bool {
    loop {
        if CPU_STATE[slot].load(Ordering::Acquire) == CPU_READY {
            return true;
        }
        if crate::arch::x86_64::tsc::rdtsc() >= deadline {
            return false;
        }
        core::hint::spin_loop();
    }
}

/// Sends INIT-SIPI-SIPI to one AP. The mailbox was filled beforehand.
fn start_ap(apic_id: u32) -> bool {
    if !crate::arch::x86_64::apic::send_init(apic_id) {
        return false;
    }
    udelay(10_000);
    if !crate::arch::x86_64::apic::send_init_deassert(apic_id) {
        return false;
    }
    udelay(10_000);
    if !crate::arch::x86_64::apic::send_sipi(apic_id, TRAMPOLINE_VECTOR) {
        return false;
    }
    udelay(200);
    if !crate::arch::x86_64::apic::send_sipi(apic_id, TRAMPOLINE_VECTOR) {
        return false;
    }
    true
}

/// Virtual address of this CPU's Local APIC MMIO window.
fn apic_lapic_virt() -> u64 {
    let base = unsafe { crate::arch::x86_64::msr::rdmsr(crate::arch::x86_64::msr::IA32_APIC_BASE) };
    crate::arch::x86_64::phys::phys_to_virt(base & 0xffff_f000)
}

/// Starts every enabled AP and collects attestations. Never fails the
/// boot: with zero working APs this is a normal single-CPU boot.
pub fn bringup(rsdp: u64) -> u32 {
    crate::println!("smp: bringup entry");
    let mut entries = [crate::acpi::CpuEntry { apic_id: 0, enabled: false }; crate::acpi::MAX_MADT_CPUS];
    let count = match crate::acpi::cpu_list(rsdp, &mut entries) {
        Ok(n) => n,
        Err(e) => {
            crate::println!("smp: no MADT CPU list ({}); single CPU", e);
            CPU_ONLINE.store(1, Ordering::Relaxed);
            return 1;
        }
    };
    crate::println!("smp: MADT lists {} CPU entries", count);
    let bsp_apic = crate::arch::x86_64::apic::apic_id();
    // Find the BSP; every other enabled entry is an AP to start.
    let mut bsp_slot = 0usize;
    let mut found_bsp = false;
    for (i, e) in entries[..count].iter().enumerate() {
        if e.apic_id == bsp_apic {
            bsp_slot = i;
            found_bsp = true;
        }
    }
    crate::println!("smp: BSP apic id {:#x}", bsp_apic);
    if !found_bsp {
        crate::println!("smp: BSP not in MADT; single CPU");
        CPU_ONLINE.store(1, Ordering::Relaxed);
        return 1;
    }
    BSP_SLOT.store(bsp_slot as u32, Ordering::Relaxed);
    /* Pin both GS bases at this CPU's per-CPU block: the kernel never
       hands GS to userspace (no ARCH_SET_GS), so no swapgs dance is
       needed and %gs always addresses per-CPU state. MSRs are per-CPU,
       so parked APs keep their own blocks without cross-CPU clobbering. */
    unsafe {
        crate::arch::x86_64::msr::wrmsr(
            crate::arch::x86_64::msr::IA32_KERNEL_GS_BASE,
            per_cpu_ptr(bsp_slot) as u64,
        );
        crate::arch::x86_64::msr::wrmsr(
            crate::arch::x86_64::msr::IA32_GS_BASE,
            per_cpu_ptr(bsp_slot) as u64,
        )
    };
    let mut ap_slots = [0usize; MAX_CPUS];
    let mut ap_count = 0usize;
    for (i, e) in entries[..count].iter().enumerate() {
        if i != bsp_slot && e.enabled && ap_count < MAX_CPUS {
            ap_slots[ap_count] = i;
            ap_count += 1;
        }
    }
    CPU_COUNT.store(count as u32, Ordering::Relaxed);
    // Record the BSP's own slot so the self-test covers it too.
    unsafe {
        let mut rsp: u64;
        core::arch::asm!("mov {}, rsp", out(reg)rsp, options(nostack, preserves_flags));
        CPU_PROBE_RSP[bsp_slot].store(rsp, Ordering::Relaxed);
    }
    CPU_MAGIC[bsp_slot].store(ATTEST_MAGIC, Ordering::Relaxed);
    CPU_COUNTER[bsp_slot].fetch_add(1, Ordering::Relaxed);
    CPU_STATE[bsp_slot].store(CPU_READY, Ordering::Release);
    unsafe {
        let ids = &raw mut TRAMP_IDS as *mut [u32; MAX_CPUS];
        (*ids)[bsp_slot] = bsp_apic;
    }
    if ap_count == 0 {
        crate::println!("smp: single CPU (apic {:#x})", bsp_apic);
        CPU_ONLINE.store(1, Ordering::Relaxed);
        return 1;
    }
    let tramp_frame = match install_trampoline() {
        Ok(f) => f,
        Err(e) => {
            crate::println!("smp: trampoline unavailable ({}); single CPU", e);
            CPU_ONLINE.store(1, Ordering::Relaxed);
            return 1;
        }
    };
    crate::println!("smp: trampoline installed frame={:#x}", tramp_frame);
    {
        let root = crate::arch::x86_64::paging::PageTableRoot::current();
        let back = root.translate(&crate::arch::x86_64::phys::Identity, TRAMPOLINE_ADDR);
        match back {
            Some(pa) => {
                let src = unsafe { &raw const trampoline_start as *const u8 };
                let first = unsafe { core::ptr::read_unaligned(src as *const u64) };
                let dstb = unsafe {
                    core::ptr::read_unaligned(
                        crate::arch::x86_64::phys::phys_to_virt(pa) as *const u64,
                    )
                };
                crate::println!("smp: trampoline check root={:#x} pa={:#x} src={:#x} first={:#x} dst={:#x}", root.pml4, pa, src as u64, first, dstb);
            }
            None => crate::println!("smp: trampoline check root={:#x} pa=unmapped", root.pml4),
        }
    }
    let root = crate::arch::x86_64::paging::PageTableRoot::current();
    let cr3 = root.pml4;
    let lapic_virt = apic_lapic_virt();
    crate::println!("smp: cr3={:#x} lapic={:#x} ap_count={}", cr3, lapic_virt, ap_count);
    // Per-AP stacks, GDT/TSS entries and the trampoline mailbox, all
    // before the first SIPI so no AP ever observes a half-built CPU.
    for k in 0..ap_count {
        let slot = ap_slots[k];
        crate::println!("smp: AP {} slot {} apic {:#x}", k, slot, entries[slot].apic_id);
        // One frame is plenty for bring-up plus the parked loop (no
        // interrupts and no syscalls run on APs yet). A single frame
        // needs no physical contiguity, unlike a multi-frame stack.
        // MS83 gives APs full scheduler stacks.
        let frame = match crate::memory::alloc_frame() {
            Some(f) => f,
            None => {
                crate::println!("smp: no stack frame for AP {}", k);
                continue;
            }
        };
        unsafe {
            crate::arch::x86_64::phys::Identity.table(frame).fill(0);
        }
        let top = crate::arch::x86_64::phys::phys_to_virt(frame) + 4096;
        crate::println!("smp: AP {} stack frame={:#x} top={:#x}", k, frame, top);
        CPU_STACK_TOP[slot].store(top, Ordering::Relaxed);
        CPU_STATE[slot].store(CPU_PRESENT, Ordering::Release);
        unsafe {
            let ids = &raw mut TRAMP_IDS as *mut [u32; MAX_CPUS];
            let stacks = &raw mut TRAMP_STACKS as *mut [u64; MAX_CPUS];
            (*ids)[slot] = entries[slot].apic_id;
            (*stacks)[slot] = top;
                // Mailbox lives in the trampoline page; address it through
                // the direct map (same physical frame).
                // NOTE: the mailbox sits at page offset 0x300 (0x7300);
                // writing at page offset 0x00 would clobber the AP's
                // real-mode entry code.
                let mb_base = crate::arch::x86_64::phys::phys_to_virt(tramp_frame) + 0x300;
                let w = |off: usize, v: u64| unsafe {
                    core::ptr::write_volatile((mb_base + off as u64) as *mut u64, v)
                };
            w(0x00, cr3);
            w(0x08, ap_main as usize as u64);
            w(0x10, lapic_virt);
            w(0x18, &raw const TRAMP_IDS as *const [u32; MAX_CPUS] as u64);
            w(0x20, &raw const TRAMP_STACKS as *const [u64; MAX_CPUS] as u64);
        }
        core::sync::atomic::fence(Ordering::SeqCst);
        if !start_ap(entries[slot].apic_id) {
            crate::println!("smp: AP {} (apic {:#x}) IPI failed", k, entries[slot].apic_id);
            CPU_STATE[slot].store(CPU_ABSENT, Ordering::Release);
        }
    }
    // Attest every started AP with a deadline; missing ones stay absent.
    let wait = wait_cycles();
    let mut online = 1u32;
    for k in 0..ap_count {
        let slot = ap_slots[k];
        if CPU_STATE[slot].load(Ordering::Acquire) == CPU_ABSENT {
            continue;
        }
        let deadline = crate::arch::x86_64::tsc::rdtsc().saturating_add(wait);
        if wait_ready(slot, deadline) {
            online += 1;
        } else {
            crate::println!(
                "smp: AP {} (apic {:#x}) timed out; trampoline stage={:#x}; continuing without it",
                k,
                entries[slot].apic_id,
                trampoline_stage(tramp_frame)
            );
            CPU_STATE[slot].store(CPU_ABSENT, Ordering::Release);
        }
    }
    CPU_ONLINE.store(online, Ordering::Relaxed);
    report();
    online
}

/// Trampoline progress marker written by the AP itself (mailbox+0x28):
/// 0x11111111 real mode, 0x22222222 protected mode, 0x33333333 long
/// mode, 0x44444444 APIC ID matched. Lets a timeout say *where* the AP
/// stopped instead of just that it never attested.
fn trampoline_stage(frame: u64) -> u32 {
    unsafe {
        let p = crate::arch::x86_64::phys::phys_to_virt(frame + 0x328) as *const u32;
        core::ptr::read_volatile(p)
    }
}
/// Human-readable attestation of every CPU that reported ready.
fn report() {
    let online = CPU_ONLINE.load(Ordering::Relaxed);
    for slot in 0..MAX_CPUS {
        if CPU_STATE[slot].load(Ordering::Acquire) != CPU_READY {
            continue;
        }
        crate::println!(
            "smp: cpu{} apic={:#x} stack_top={:#x} rsp={:#x} ticks={}",
            slot,
            apic_id_of_slot(slot),
            CPU_STACK_TOP[slot].load(Ordering::Relaxed),
            CPU_PROBE_RSP[slot].load(Ordering::Relaxed),
            CPU_COUNTER[slot].load(Ordering::Relaxed),
        );
    }
    crate::println!("smp: {} CPUs online", online);
}

/// Self-test: every READY slot must show the attestation magic, exactly
/// one counter tick, and (for APs) a stack probe inside its own stack
/// range. Runs on the BSP after bringup; failures halt the boot loudly
/// rather than limping on with half-initialized CPUs.
pub fn self_test() -> Result<u32, &'static str> {
    let mut ok = 0u32;
    for slot in 0..MAX_CPUS {
        if CPU_STATE[slot].load(Ordering::Acquire) != CPU_READY {
            continue;
        }
        if CPU_MAGIC[slot].load(Ordering::Relaxed) != ATTEST_MAGIC {
            return Err("smp self-test: attestation magic mismatch");
        }
        if CPU_COUNTER[slot].load(Ordering::Relaxed) != 1 {
            return Err("smp self-test: attestation counter != 1");
        }
        if slot as u32 == BSP_SLOT.load(Ordering::Relaxed) {
            // The BSP attests with its live stack pointer instead.
            if CPU_PROBE_RSP[slot].load(Ordering::Relaxed) == 0 {
                return Err("smp self-test: BSP stack probe missing");
            }
        } else {
            let rsp = CPU_PROBE_RSP[slot].load(Ordering::Relaxed);
            let top = CPU_STACK_TOP[slot].load(Ordering::Relaxed);
            if top == 0 || rsp == 0 || rsp > top || rsp <= top.wrapping_sub(AP_STACK_SIZE as u64) {
                return Err("smp self-test: AP stack probe outside its stack");
            }
            if apic_id_of_slot(slot) == 0xFFFF_FFFF {
                return Err("smp self-test: AP without table entry");
            }
        }
        ok += 1;
    }
    if ok != CPU_ONLINE.load(Ordering::Relaxed) {
        return Err("smp self-test: online count disagrees with attestations");
    }
    if ok == 0 {
        return Err("smp self-test: no CPU attested");
    }
    crate::println!("smp: self-test passed ({} attested)", ok);
    Ok(ok)
}

/// Per-CPU data block, addressed through GS in kernel mode during syscalls.
/// Layout must match the offsets hard-coded in arch/x86_64/syscall.S.
/// Fields are only appended (never reordered): the assembly offsets of the
/// syscall path must stay stable.
#[repr(C)]
pub struct PerCpuData {
    pub current_fpu: u64,
    pub syscall_kernel_rsp: u64,
    pub syscall_user_rsp: u64,
    pub syscall_frame: u64,
    pub syscall_result: u64,
    pub exec_rip: u64,
    pub sigret_value: u64,
    pub user_cs: u64,
    pub user_ss: u64,
    pub sigret_valid: u8,
    pub exec_pending: u8,
    pub _pad: [u8; 6],
    pub cpu_index: u64,
    pub diag_entry_rsp: u64,
    /* Set by interrupt return paths when the interrupted user task has a
       stop pending; the asm return path then sets TF so the task traps
       into #DB, where the stop runs on its own kernel stack. IST handlers
       cannot schedule directly (shared per-CPU stack). */
    pub stop_trap: u8,
    pub _pad2: [u8; 7],
    /* While set, live XMM holds kernel scratch and the task's user SSE
       state is parked in its FPU area: a context switch must NOT save over
       it, but divert the save to scratch. Set at syscall entry (after the
       park), cleared at return (after the restore). Parked per task across
       switches like user_rsp so migration is safe. */
    pub fpu_parked: u8,
    pub _pad3: [u8; 7],
}

impl PerCpuData {
    pub const fn zero() -> Self {
        Self {
            current_fpu: 0,
            syscall_kernel_rsp: 0,
            syscall_user_rsp: 0,
            syscall_frame: 0,
            syscall_result: 0,
            exec_rip: 0,
            sigret_value: 0,
            user_cs: 0,
            user_ss: 0,
            sigret_valid: 0,
            exec_pending: 0,
            _pad: [0; 6],
            cpu_index: 0,
            diag_entry_rsp: 0,
            stop_trap: 0,
            _pad2: [0; 7],
            fpu_parked: 0,
            _pad3: [0; 7],
        }
    }
}

static mut PER_CPU: [PerCpuData; MAX_CPUS] = [const { PerCpuData::zero() }; MAX_CPUS];

pub fn per_cpu_ptr(slot: usize) -> *mut PerCpuData {
    if slot >= MAX_CPUS {
        return core::ptr::null_mut();
    }
    unsafe { &raw mut PER_CPU[slot] }
}

/// Index of the running CPU in the MADT table (0..MAX_CPUS).
pub fn current_cpu() -> usize {
    let id = crate::arch::x86_64::apic::apic_id();
    for i in 0..MAX_CPUS {
        if CPU_STATE[i].load(core::sync::atomic::Ordering::Acquire) == CPU_READY {
            let stored = unsafe { core::ptr::read_volatile(&raw const TRAMP_IDS[i]) };
            if stored == id {
                return i;
            }
        }
    }
    BSP_SLOT.load(core::sync::atomic::Ordering::Relaxed) as usize
}
