#[repr(C,packed)]#[derive(Clone,Copy)]struct Gate{lo:u16,sel:u16,ist:u8,attr:u8,mid:u16,hi:u32,zero:u32}
impl Gate{const fn missing()->Self{Self{lo:0,sel:0,ist:0,attr:0,mid:0,hi:0,zero:0}}fn set(&mut self,f:usize,ist:u8){self.lo=f as u16;self.sel=super::descriptors::KCODE;self.ist=ist&7;self.attr=0x8e;self.mid=(f>>16)as u16;self.hi=(f>>32)as u32;self.zero=0;}}
#[repr(C,packed)]struct Ptr{limit:u16,base:u64}static mut IDT:[Gate;256]=[Gate::missing();256];
unsafe extern "sysv64"{fn apic_timer_entry();fn isr_default();fn isr_page_fault();fn isr_double_fault();fn isr_general_protection();fn isr_invalid_tss();fn isr_segment_not_present();fn isr_stack_segment();fn isr_alignment_check();}
pub unsafe fn init(){unsafe{for i in 0..256{(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(i)).set(isr_default as usize,0)}(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(8)).set(isr_double_fault as usize,1);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(10)).set(isr_invalid_tss as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(11)).set(isr_segment_not_present as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(12)).set(isr_stack_segment as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(13)).set(isr_general_protection as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(14)).set(isr_page_fault as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(17)).set(isr_alignment_check as usize,0);(*core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(super::apic::TIMER_VECTOR as usize)).set(apic_timer_entry as usize,2);let p=Ptr{limit:(core::mem::size_of::<[Gate;256]>()-1)as u16,base:core::ptr::addr_of!(IDT)as u64};core::arch::asm!("lidt [{}]",in(reg)&p,options(readonly,nostack));}}
#[unsafe(no_mangle)]pub extern "sysv64" fn exception_rust(vector:u64,error:u64,rip:u64,cs:u64){
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
 crate::println!("kernel exception {} error={:#x} rip={:#x} cs={:#x} cr2={:#x}",vector,error,rip,cs,cr2);
 crate::arch::halt()
}
