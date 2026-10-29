//! Slop-native interrupt core: descriptor table, handler registry, dispatch.
//!
//! OS-neutral and shared by Slop drivers and the Linux compat adapter.
//! Real delivery today is via LAPIC IPI to Slop test vectors (proves
//! register → interrupt → dispatch → handler → unregister end to end);
//! IOAPIC/MSI routing hooks in later (mask/unmask already abstracted).
//! All registry ops are IRQ-safe; dispatch runs with IF=0 on the
//! interrupted stack. SMP-safe via lock + in-flight refcount so unload
//! can wait out running handlers instead of freeing live code.
use crate::sync::IrqSpinLock;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const MAX_HANDLERS_PER_IRQ: usize = 4;
pub const MAX_IRQ: usize = 256;

pub type IrqHandler = fn(vector: u8, dev_id: u64) -> bool;

#[derive(Clone, Copy)]
struct IrqAction {
    used: bool,
    handler: Option<IrqHandler>,
    dev_id: u64,
    name: [u8; 32],
    shared: bool,
}
const NO_ACTION: IrqAction = IrqAction { used: false, handler: None, dev_id: 0, name: [0; 32], shared: false };

struct IrqDesc {
    actions: [IrqAction; MAX_HANDLERS_PER_IRQ],
    masked: bool,
    handled: u64,
    spurious: u64,
    in_flight: AtomicU32,
}
impl IrqDesc {
    const fn new() -> Self {
        Self { actions: [NO_ACTION; MAX_HANDLERS_PER_IRQ], masked: false, handled: 0, spurious: 0, in_flight: AtomicU32::new(0) }
    }
}

static DESCS: IrqSpinLock<[IrqDesc; MAX_IRQ]> = IrqSpinLock::new([const { IrqDesc::new() }; MAX_IRQ]);

fn copy_name(dst: &mut [u8; 32], src: &[u8]) {
    let n = core::cmp::min(dst.len() - 1, src.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n] = 0;
}

/// Register a handler. Fails with BUSY if the vector is taken non-shared,
/// NOENT if the table slot is invalid, NOMEM if the action list is full.
pub fn register(vector: u8, handler: IrqHandler, dev_id: u64, name: &[u8], shared: bool) -> i32 {
    let mut t = DESCS.lock();
    let d = &mut t[vector as usize];
    for a in d.actions.iter() {
        if a.used && (!a.shared || !shared) { return -16; } // -EBUSY
    }
    match d.actions.iter_mut().find(|a| !a.used) {
        Some(a) => {
            a.used = true;
            a.handler = Some(handler);
            a.dev_id = dev_id;
            copy_name(&mut a.name, name);
            a.shared = shared;
            0
        }
        None => -12, // -ENOMEM
    }
}

/// Unregister by (vector, dev_id). Waits (bounded spin) for in-flight
/// handlers on SMP so module unload never frees running code.
pub fn unregister(vector: u8, dev_id: u64) -> i32 {
    let idx = {
        let mut t = DESCS.lock();
        let d = &mut t[vector as usize];
        let mut pos = None;
        for (i, a) in d.actions.iter().enumerate() {
            if a.used && a.dev_id == dev_id { pos = Some(i); break; }
        }
        match pos {
            Some(i) => { d.actions[i] = NO_ACTION; i }
            None => return -2, // -ENOENT
        }
    };
    // Wait out in-flight handlers (bounded; UP never blocks here).
    let mut spins = 0u32;
    loop {
        let n = DESCS.lock()[vector as usize].in_flight.load(Ordering::Acquire);
        if n == 0 { break; }
        core::hint::spin_loop();
        spins += 1;
        if spins > 10_000_000 { break; }
    }
    let _ = idx;
    0
}

pub fn mask(vector: u8) {
    DESCS.lock()[vector as usize].masked = true;
}
pub fn unmask(vector: u8) {
    DESCS.lock()[vector as usize].masked = false;
}

/// Dispatch: called from the IRQ entry stub with IF=0. Runs each action in
/// registration order until one claims it (shared semantics). Spurious and
/// masked interrupts are counted, not delivered.
pub fn dispatch(vector: u8) -> bool {
    // Snapshot actions under lock, run without holding it (handlers may
    // register/unregister other vectors; never this one synchronously).
    let (handlers, masked) = {
        let t = DESCS.lock();
        let d = &t[vector as usize];
        let mut hs = [(None, 0u64); MAX_HANDLERS_PER_IRQ];
        for (i, a) in d.actions.iter().enumerate() {
            if a.used { hs[i] = (a.handler, a.dev_id); }
        }
        (hs, d.masked)
    };
    if masked { return false; }
    DESCS.lock()[vector as usize].in_flight.fetch_add(1, Ordering::AcqRel);
    let mut claimed = false;
    for (h, dev) in handlers.iter() {
        if let Some(f) = h {
            if f(vector, *dev) { claimed = true; break; }
        }
    }
    {
        let mut t = DESCS.lock();
        let d = &mut t[vector as usize];
        d.in_flight.fetch_sub(1, Ordering::AcqRel);
        if claimed { d.handled += 1; } else { d.spurious += 1; }
    }
    claimed
}

pub fn stats(vector: u8) -> (u64, u64) {
    let t = DESCS.lock();
    let d = &t[vector as usize];
    (d.handled, d.spurious)
}

static TEST_FIRED: AtomicU64 = AtomicU64::new(0);
static MSI_FIRED: AtomicU64 = AtomicU64::new(0);
static INTX_FIRED: AtomicU64 = AtomicU64::new(0);
fn test_handler(vector: u8, _dev: u64) -> bool {
    if vector == crate::arch::x86_64::apic::TEST_VECTOR {
        TEST_FIRED.store(0x4952515445535455, Ordering::SeqCst); // "IRQTESTU"
        return true;
    }
    false
}
fn msi_test_handler(vector: u8, _dev: u64) -> bool {
    if vector >= crate::arch::x86_64::apic::MSI_VECTOR_BASE
        && vector < crate::arch::x86_64::apic::MSI_VECTOR_BASE + crate::arch::x86_64::apic::MSI_VECTOR_COUNT
    {
        MSI_FIRED.store(0x4d534954455354, Ordering::SeqCst); // "MSITEST"
        return true;
    }
    false
}
fn intx_test_handler(vector: u8, _dev: u64) -> bool {
    if vector == crate::arch::x86_64::apic::INTX_VECTOR {
        INTX_FIRED.store(0x494e545855545354, Ordering::SeqCst); // "INTXUTST"
        return true;
    }
    false
}

/// Boot self-test: register → self-IPI → dispatch → handler → unregister.
/// Proves the Slop IRQ core vertical without hardware.
pub fn selftest_irq_dispatch() {
    use crate::arch::x86_64::apic;
    // Boot runs with IF clear; IPI delivery needs IF set. Enable for the
    // test window and restore afterwards.
    let flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nomem)) };
    let was_clear = flags & (1 << 9) == 0;
    if was_clear { unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) }; }
    TEST_FIRED.store(0, Ordering::SeqCst);
    if register(apic::TEST_VECTOR, test_handler, 0x5a5a5a5, b"slotest\0", false) != 0 {
        crate::println!("irq: selftest register failed");
        return;
    }
    let me = apic::apic_id();
    if !apic::send_fixed_ipi(me, apic::TEST_VECTOR) {
        crate::println!("irq: selftest IPI send failed");
        unregister(apic::TEST_VECTOR, 0x5a5a5a5);
        return;
    }
    // Wait for the handler (bounded; IPI delivery is prompt on UP/SMP).
    let mut spins = 0u32;
    while TEST_FIRED.load(Ordering::SeqCst) != 0x4952515445535455 {
        core::hint::spin_loop();
        spins += 1;
        if spins > 50_000_000 {
            break;
        }
    }
    let (handled, spurious) = stats(apic::TEST_VECTOR);
    unregister(apic::TEST_VECTOR, 0x5a5a5a5);
    if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
    if TEST_FIRED.load(Ordering::SeqCst) == 0x4952515445535455 {
        crate::println!("irq: selftest PASS (handled={} spurious={})", handled, spurious);
    } else {
        crate::println!("irq: selftest FAIL no dispatch (handled={} spurious={})", handled, spurious);
    }
}

/// MSI IDT/stub self-test: self-IPI to MSI_VECTOR_BASE proves the new
/// gates dispatch through the shared core (no hardware needed).
pub fn selftest_msi_dispatch() {
    use crate::arch::x86_64::apic;
    let flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nomem)) };
    let was_clear = flags & (1 << 9) == 0;
    if was_clear { unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) }; }
    MSI_FIRED.store(0, Ordering::SeqCst);
    if register(apic::MSI_VECTOR_BASE, msi_test_handler, 0x5a5a5b1, b"msitest\0", false) != 0 {
        crate::println!("irq: msi selftest register failed");
        if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
        return;
    }
    let me = apic::apic_id();
    if !apic::send_fixed_ipi(me, apic::MSI_VECTOR_BASE) {
        crate::println!("irq: msi selftest IPI send failed");
        unregister(apic::MSI_VECTOR_BASE, 0x5a5a5b1);
        if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
        return;
    }
    let mut spins = 0u32;
    while MSI_FIRED.load(Ordering::SeqCst) != 0x4d534954455354 {
        core::hint::spin_loop();
        spins += 1;
        if spins > 50_000_000 { break; }
    }
    let (handled, spurious) = stats(apic::MSI_VECTOR_BASE);
    unregister(apic::MSI_VECTOR_BASE, 0x5a5a5b1);
    if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
    if MSI_FIRED.load(Ordering::SeqCst) == 0x4d534954455354 {
        crate::println!("irq: msi selftest PASS (handled={} spurious={})", handled, spurious);
    } else {
        crate::println!("irq: msi selftest FAIL no dispatch (handled={} spurious={})", handled, spurious);
    }
}

/// INTx vector gate check: self-IPI to 0x58 through the shared core.
pub fn selftest_intx_dispatch() {
    use crate::arch::x86_64::apic;
    let flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nomem)) };
    let was_clear = flags & (1 << 9) == 0;
    if was_clear { unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) }; }
    INTX_FIRED.store(0, Ordering::SeqCst);
    if register(apic::INTX_VECTOR, intx_test_handler, 0x5a5a5c1, b"intxtest\0", false) != 0 {
        crate::println!("irq: intx selftest register failed");
        if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
        return;
    }
    let me = apic::apic_id();
    if !apic::send_fixed_ipi(me, apic::INTX_VECTOR) {
        crate::println!("irq: intx selftest IPI send failed");
        unregister(apic::INTX_VECTOR, 0x5a5a5c1);
        if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
        return;
    }
    let mut spins = 0u32;
    while INTX_FIRED.load(Ordering::SeqCst) != 0x494e545855545354 {
        core::hint::spin_loop();
        spins += 1;
        if spins > 50_000_000 { break; }
    }
    let (handled, spurious) = stats(apic::INTX_VECTOR);
    unregister(apic::INTX_VECTOR, 0x5a5a5c1);
    if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
    if INTX_FIRED.load(Ordering::SeqCst) == 0x494e545855545354 {
        crate::println!("irq: intx selftest PASS (handled={} spurious={})", handled, spurious);
    } else {
        crate::println!("irq: intx selftest FAIL no dispatch (handled={} spurious={})", handled, spurious);
    }
}
