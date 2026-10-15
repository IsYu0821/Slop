//! Lifetime of the single bootstrap Linux process.
use crate::{arch::x86_64::{paging::PageTableRoot,phys::Identity},sync::SpinLock};
#[derive(Clone,Copy)]struct State{bootstrap_cr3:u64,process_cr3:u64,image_slot:usize,stack_slot:usize,active:bool,exit_status:i32,term_vector:u64}
static STATE:SpinLock<State>=SpinLock::new(State{bootstrap_cr3:0,process_cr3:0,image_slot:0,stack_slot:0,active:false,exit_status:0,term_vector:0});
pub fn install(bootstrap_cr3:u64,process_cr3:u64,image_slot:usize,stack_slot:usize){*STATE.lock()=State{bootstrap_cr3,process_cr3,image_slot,stack_slot,active:true,exit_status:0,term_vector:0};}
pub fn is_active()->bool{STATE.lock().active}
pub fn terminate_exit(status:i32)->!{{let mut s=STATE.lock();s.exit_status=status;}crate::println!("task exited status={}",status);reap_current();crate::task::finish_current(status)}
pub fn terminate_fault(vector:u64,error:u64,rip:u64,cr2:u64)->!{{let mut s=STATE.lock();s.exit_status=128+11;s.term_vector=vector;}crate::println!("user fault vector={} error={:#x} rip={:#x} cr2={:#x}",vector,error,rip,cr2);reap_current();crate::task::finish_current(128+11)}
pub fn reap_current(){
 let mut s=STATE.lock();if !s.active{return}
 let bootstrap=PageTableRoot{pml4:s.bootstrap_cr3};unsafe{bootstrap.activate()}
 let proc=PageTableRoot{pml4:s.process_cr3};let phys=Identity;
 if proc.destroy_private_slot(&phys,s.image_slot).is_err(){crate::println!("warning: image slot teardown failed")}
 if proc.destroy_private_slot(&phys,s.stack_slot).is_err(){crate::println!("warning: stack slot teardown failed")}
 if crate::memory::free_frame(s.process_cr3).is_err(){crate::println!("warning: process PML4 recycle failed")}
 s.active=false;
 crate::println!("process address space reclaimed status={} vector={}",s.exit_status,s.term_vector);
}
pub fn bootstrap_cr3()->u64{STATE.lock().bootstrap_cr3}
pub fn replace_image(new_cr3:u64,image_slot:usize,stack_slot:usize)->Result<(),i64>{
 let old={let s=STATE.lock();if !s.active{return Err(crate::errno::EINVAL)}(s.process_cr3,s.image_slot,s.stack_slot,s.bootstrap_cr3)};
 unsafe{PageTableRoot{pml4:new_cr3}.activate()}
 crate::task::bind_current_address_space(new_cr3,crate::task::current_kernel_stack_top());
 {let mut s=STATE.lock();s.process_cr3=new_cr3;s.image_slot=image_slot;s.stack_slot=stack_slot;s.bootstrap_cr3=old.3;}
 let root=PageTableRoot{pml4:old.0};let phys=Identity;
 if root.destroy_private_slot(&phys,old.1).is_err(){crate::println!("warning: old exec image teardown failed")}
 if old.2!=old.1&&root.destroy_private_slot(&phys,old.2).is_err(){crate::println!("warning: old exec stack teardown failed")}
 if crate::memory::free_frame(old.0).is_err(){crate::println!("warning: old exec PML4 recycle failed")}Ok(())
}
