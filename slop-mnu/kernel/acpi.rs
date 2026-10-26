//! Minimal ACPI table discovery for interrupt controllers.
#[repr(C,packed)]struct Rsdp1{sig:[u8;8],checksum:u8,oem:[u8;6],revision:u8,rsdt:u32}
#[repr(C,packed)]struct Rsdp2{v1:Rsdp1,length:u32,xsdt:u64,extended_checksum:u8,reserved:[u8;3]}
#[repr(C,packed)]pub struct SdtHeader{pub signature:[u8;4],pub length:u32,revision:u8,checksum:u8,oem_id:[u8;6],oem_table_id:[u8;8],oem_revision:u32,creator_id:u32,creator_revision:u32}
#[derive(Clone,Copy)]pub struct InterruptInfo{pub lapic:u64,pub ioapic:u64,pub ioapic_gsi:u32,pub iso_gsi:[u32;16],pub iso_flags:[u16;16]}
/// One MADT processor entry: the APIC ID the CPU answers to and whether
/// firmware marks it enabled (bootable now) as opposed to merely
/// hot-plug capable.
#[derive(Clone,Copy)]pub struct CpuEntry{pub apic_id:u32,pub enabled:bool}
pub const MAX_MADT_CPUS:usize=64;
fn sum(ptr:*const u8,len:usize)->u8{let mut v=0u8;for i in 0..len{v=v.wrapping_add(unsafe{ptr.add(i).read_unaligned()});}v}
unsafe fn header(pa:u64)->Option<&'static SdtHeader>{if pa==0{return None}let h=unsafe{&*(pa as*const SdtHeader)};if h.length<core::mem::size_of::<SdtHeader>()as u32||sum(pa as*const u8,h.length as usize)!=0{None}else{Some(h)}}
pub unsafe fn interrupt_info(rsdp_pa:u64)->Result<InterruptInfo,&'static str>{if rsdp_pa==0{return Err("RSDP missing")}let r=unsafe{&*(rsdp_pa as*const Rsdp2)};if &r.v1.sig!=b"RSD PTR "||sum(rsdp_pa as*const u8,20)!=0{return Err("invalid RSDP")}let(use_xsdt,root_pa)=(r.v1.revision>=2&&r.length>=36&&sum(rsdp_pa as*const u8,r.length as usize)==0,r.xsdt);let root_pa=if use_xsdt{root_pa}else{r.v1.rsdt as u64};let root=unsafe{header(root_pa)}.ok_or("invalid ACPI root")?;let es=if use_xsdt{8}else{4};let count=(root.length as usize-core::mem::size_of::<SdtHeader>())/es;let base=(root_pa as usize+core::mem::size_of::<SdtHeader>())as*const u8;let mut madt=0;for i in 0..count{let p=unsafe{if es==8{base.add(i*8).cast::<u64>().read_unaligned()}else{base.add(i*4).cast::<u32>().read_unaligned()as u64}};if let Some(h)=unsafe{header(p)}{if &h.signature==b"APIC"{madt=p;break}}}if madt==0{return Err("MADT missing")}parse_madt(madt)}
unsafe fn parse_madt(pa:u64)->Result<InterruptInfo,&'static str>{let h=unsafe{header(pa)}.ok_or("invalid MADT")?;let lapic=unsafe{((pa+core::mem::size_of::<SdtHeader>()as u64)as*const u32).read_unaligned()}as u64;let mut out=InterruptInfo{lapic,ioapic:0,ioapic_gsi:0,iso_gsi:[0;16],iso_flags:[0;16]};for i in 0..16{out.iso_gsi[i]=i as u32}let mut p=pa as usize+core::mem::size_of::<SdtHeader>()+8;let end=pa as usize+h.length as usize;while p+2<=end{let typ=unsafe{*(p as*const u8)};let len=unsafe{*((p+1)as*const u8)}as usize;if len<2||p+len>end{return Err("corrupt MADT entry")}match typ{1 if len>=12=>{out.ioapic=unsafe{((p+4)as*const u32).read_unaligned()}as u64;out.ioapic_gsi=unsafe{((p+8)as*const u32).read_unaligned()};},2 if len>=10=>{let src=unsafe{*((p+3)as*const u8)}as usize;if src<16{out.iso_gsi[src]=unsafe{((p+4)as*const u32).read_unaligned()};out.iso_flags[src]=unsafe{((p+8)as*const u16).read_unaligned()};}},5 if len>=12=>out.lapic=unsafe{((p+4)as*const u64).read_unaligned()},_=>{}}p+=len;}  if out.ioapic==0{return Err("IOAPIC missing")}Ok(out)}
/* Lists the MADT processor entries (type 0 local APIC and type 9 x2APIC)
   in firmware order, capped so the per-CPU tables stay bounded. */
pub fn cpu_list(rsdp_pa:u64,out:&mut [CpuEntry;MAX_MADT_CPUS])->Result<usize,&'static str>{
 // NOTE: this runs after the high-half transition, where low memory is no
 // longer identity-mapped. Every firmware physical address must go through
 // the direct map before dereferencing.
 let pv=|pa:u64|crate::arch::x86_64::phys::phys_to_virt(pa);
 if rsdp_pa==0{return Err("RSDP missing")}
 let r=unsafe{&*(pv(rsdp_pa) as *const Rsdp2)};
 if &r.v1.sig!=b"RSD PTR "{return Err("RSDP missing")}
 let(use_xsdt,root_pa)=(r.v1.revision>=2&&r.length>=36&&sum(pv(rsdp_pa) as *const u8,r.length as usize)==0,r.xsdt);
 let root_pa=if use_xsdt{root_pa}else{r.v1.rsdt as u64};
 let root=unsafe{header(pv(root_pa))}.ok_or("invalid ACPI root")?;
 let es=if use_xsdt{8}else{4};
 let count=(root.length as usize-core::mem::size_of::<SdtHeader>())/es;
 let base=(pv(root_pa) as usize+core::mem::size_of::<SdtHeader>())as*const u8;
 let mut madt:u64=0;
 for i in 0..count{
  let raw=unsafe{if es==8{base.add(i*es).cast::<u64>().read_unaligned()}else{base.add(i*4).cast::<u32>().read_unaligned() as u64}};
  match unsafe{header(pv(raw))}{Some(h)=>{if &h.signature==b"APIC"{madt=raw;break}},None=>{}}
 }
 if madt==0{return Err("MADT missing")}
 let h=unsafe{header(pv(madt))}.ok_or("invalid MADT")?;
 let mut p=madt as usize+core::mem::size_of::<SdtHeader>()+8;
 let end=madt as usize+h.length as usize;
 let mut n=0usize;
 while p+2<=end&&n<MAX_MADT_CPUS{
  let vp=pv(p as u64) as usize;
  let typ=unsafe{*(vp as *const u8)};
  let len=unsafe{*((vp+1) as *const u8)}as usize;
  if len<2||p+len>end{break}
  match typ{
   0 if len>=8=>{
    let apic_id=unsafe{*((vp+3) as *const u8)} as u32;
    let flags=unsafe{core::ptr::read_unaligned((vp+4) as *const u32)};
    out[n]=CpuEntry{apic_id,enabled:flags&1!=0};n+=1;
   }
   9 if len>=16=>{
    let apic_id=unsafe{core::ptr::read_unaligned((vp+4) as *const u32)};
    let flags=unsafe{core::ptr::read_unaligned((vp+8) as *const u32)};
    out[n]=CpuEntry{apic_id,enabled:flags&1!=0};n+=1;
   }
   _=>{},
  }
  p+=len;
 }
 Ok(n)
}