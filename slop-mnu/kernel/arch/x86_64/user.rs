use super::{descriptors,syscall::UserFrame};
unsafe extern "sysv64"{fn enter_user_asm(rip:u64,rsp:u64,rflags:u64,cs:u64,ss:u64)->!;fn resume_user_asm(frame:*const UserFrame,cs:u64,ss:u64)->!;}
pub unsafe fn enter(rip:u64,rsp:u64)->!{
 if !canonical_user(rip)||!canonical_user(rsp){crate::println!("invalid user entry context");crate::arch::halt()}
 unsafe{enter_user_asm(rip,rsp,0x202,descriptors::UCODE as u64,descriptors::UDATA as u64)}
}
pub unsafe fn resume(frame:&UserFrame)->!{
 if !canonical_user(frame.rip)||!canonical_user(frame.rsp){crate::println!("invalid fork resume context rip={:#x} rsp={:#x}",frame.rip,frame.rsp);crate::arch::halt()}
 unsafe{resume_user_asm(frame,descriptors::UCODE as u64,descriptors::UDATA as u64)}
}
pub fn canonical_user(v:u64)->bool{v>=crate::usercopy::USER_MIN as u64&&v<crate::usercopy::USER_MAX as u64}
