#[repr(C,packed)]#[derive(Clone,Copy)]struct Gate{lo:u16,sel:u16,ist:u8,attr:u8,mid:u16,hi:u32,zero:u32}
impl Gate{const fn missing()->Self{Self{lo:0,sel:0,ist:0,attr:0,mid:0,hi:0,zero:0}}fn set(&mut self,f:usize,ist:u8){self.lo=f as u16;self.sel=super::descriptors::KCODE;self.ist=ist&7;self.attr=0x8e;self.mid=(f>>16)as u16;self.hi=(f>>32)as u32;self.zero=0;}}
#[repr(C,packed)]struct Ptr{limit:u16,base:u64}static mut IDT:[Gate;256]=[Gate::missing();256];unsafe extern "sysv64"{fn apic_timer_entry();fn apic_resched_entry();fn isr_default();fn isr_page_fault();fn isr_double_fault();fn isr_general_protection();fn isr_invalid_tss();fn isr_segment_not_present();fn isr_stack_segment();fn isr_alignment_check();}
pub unsafe fn init(){unsafe{for i in 0..256{(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(i)).set(isr_default as usize,0)}(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(8)).set(isr_double_fault as usize,1);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(10)).set(isr_invalid_tss as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(11)).set(isr_segment_not_present as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(12)).set(isr_stack_segment as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(13)).set(isr_general_protection as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(14)).set(isr_page_fault as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(17)).set(isr_alignment_check as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(super::apic::TIMER_VECTOR as usize)).set(apic_timer_entry as usize,2);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(super::apic::RESCHED_VECTOR as usize)).set(apic_resched_entry as usize,2);let p=Ptr{limit:(core::mem::size_of::<[Gate;256]>()-1)as u16,base:core::ptr::addr_of!(IDT)as u64};core::arch::asm!("lidt [{}]",in(reg)&p,options(readonly,nostack));}}
#[unsafe(no_mangle)]pub extern "sysv64" fn exception_rust(vector:u64,error:u64,rip:u64,cs:u64,frame:u64){
 /* DIAG-MS93: record every exception before taking any locks (a fault while
    holding SCHED would self-deadlock below and leave no trace). */
 crate::println!("diag exc vector={} err={:#x} rip={:#x} cs={:#x} apic={:#x}",vector,error,rip,cs,unsafe{crate::arch::x86_64::apic::apic_id()});
 let cr2:u64;unsafe{core::arch::asm!("mov {}, cr2",out(reg)cr2,options(nostack,preserves_flags));}
 /* Demand paging and COW also fault while the kernel is copying to or from
    userspace, so user-address #PF is recovered from either ring. */
 if vector==14 && (cr2 as usize)<crate::usercopy::USER_MAX && crate::task::has_current_process() {
   let write=error & 2 != 0;
   let handled=crate::task::with_current_process(|p|p.handle_page_fault(cr2,write).is_ok()).unwrap_or(false);
   if handled { return; }
 }
 if cs&3==3 && crate::task::has_current_process() {
   crate::task::exit_fault(vector,error,rip,cr2)
 }
  /* Spurious LAPIC interrupts (notably around INIT/SIPI) carry no work:
     acknowledge nothing, halt nothing, just resume. */
  if vector==super::apic::SPURIOUS_VECTOR as u64 {
    return;
  }
  /* DIAG-MS93: dump the faulting IRET frame words. */
  if vector==13 {
    let f=frame as *const u64;
    let (w0,w1,w2,w3,w4,w5,w6,w7)=unsafe{(core::ptr::read_volatile(f),core::ptr::read_volatile(f.add(1)),core::ptr::read_volatile(f.add(2)),core::ptr::read_volatile(f.add(3)),core::ptr::read_volatile(f.add(4)),core::ptr::read_volatile(f.add(5)),core::ptr::read_volatile(f.add(6)),core::ptr::read_volatile(f.add(7)))};
    let cpu=crate::smp::current_cpu();
    let entry_rsp=unsafe{(*crate::smp::per_cpu_ptr(cpu)).diag_entry_rsp};
    let ist1=crate::smp::tss_diag(cpu).2;
    crate::println!("diag gp frame={:#x} err={:#x} w0={:#x} w1={:#x} w2={:#x} w3={:#x} w4={:#x} w5={:#x} w6={:#x} w7={:#x}",frame,error,w0,w1,w2,w3,w4,w5,w6,w7);
    crate::println!("diag apic={:#x} entry_rsp={:#x} ist1={:#x}",crate::arch::x86_64::apic::apic_id(),entry_rsp,ist1);
  }
  /* DIAG-MS93: dump task table on a null-RIP fetch fault. */
  if vector==14 && rip==0 {
    let sp:u64;unsafe{core::arch::asm!("mov {}, rsp",out(reg)sp,options(nostack,preserves_flags))};
    crate::println!("diag nullrip rsp={:#x} cpu={} pid={}",sp,crate::smp::current_cpu(),crate::task::current_pid());
    crate::task::diag_tasks();
    crate::task::diag_stacks(sp);
    crate::task::diag_ap_progress();
    /* Fault rsp is above the handler frame: dump the faulting call chain. */
    let fr=sp.wrapping_add(448);
    for i in 0..32u64 {
      let v=unsafe{core::ptr::read_volatile(fr.wrapping_add(i*8) as *const u64)};
      if v!=0 {
        crate::println!("diag fra+{}={:#x}",i,v);
      }
    }
  }
 /* The bytes of the faulting instruction, so the place can be found.
    The RIP itself may point at unmapped memory (fetch fault), in which
    case reading it would raise a nested fault and hide the true RIP. */
 let code: &[u8] = if crate::arch::x86_64::phys::virt_to_phys(rip & !0xfff) != 0 {
   unsafe { core::slice::from_raw_parts(rip as *const u8, 8) }
 } else {
   &[]
 };
 let apic=unsafe{crate::arch::x86_64::apic::apic_id()};
 crate::println!("kernel exception {} error={:#x} rip={:#x} cs={:#x} cr2={:#x} apic={:#x} code={:02x?}",vector,error,rip,cs,cr2,apic,code);
 crate::arch::halt()
}

/* Reloads the shared table on another CPU. The handlers are written for
   any CPU to run: faults print diagnostics and halt, so a parked AP can
   safely point at the same IDT once its own GDT/TSS are live. */
pub unsafe fn reload(){
 unsafe{
  let p=Ptr{limit:(core::mem::size_of::<[Gate;256]>()-1)as u16,base:core::ptr::addr_of!(IDT) as u64};
  core::arch::asm!("lidt [{}]",in(reg)&p,options(readonly,nostack));
 }
}
