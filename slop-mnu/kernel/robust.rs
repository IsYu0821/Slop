//! Robust-futex list processing at task exit.
//! The kernel walks the exiting task's robust list in user memory; every
//! futex word still owned by the task (low 30 bits equal to the exiting pid)
//! is marked FUTEX_OWNER_DIED, the waiters' bit is preserved, and one waiter
//! is woken so it can observe the death and finish its unlock protocol.
//! The walk aborts on the first unreadable word, so a corrupted list cannot
//! loop or fault in the kernel.
const FUTEX_TID_MASK:u32=0x3fff_ffff;
const FUTEX_OWNER_DIED:u32=0x4000_0000;
const FUTEX_WAITERS:u32=0x8000_0000;
const MAX_ENTRIES:usize=64;

fn canonical(addr:u64,len:u64)->bool{
 match addr.checked_add(len){Some(end)=>addr>=crate::usercopy::USER_MIN as u64&&end<=crate::usercopy::USER_MAX as u64,None=>false}
}
fn read_u64(addr:u64)->Result<u64,i64>{
 if !canonical(addr,8){return Err(crate::errno::EFAULT)}
 let mut b=[0u8;8];
 crate::usercopy::copy_from_user(&mut b,addr as usize)?;
 Ok(u64::from_ne_bytes(b))
}
fn read_u32(addr:u64)->Result<u32,i64>{
 if !canonical(addr,4){return Err(crate::errno::EFAULT)}
 let mut b=[0u8;4];
 crate::usercopy::copy_from_user(&mut b,addr as usize)?;
 Ok(u32::from_ne_bytes(b))
}
fn write_u32(addr:u64,v:u32)->Result<(),i64>{
 if !canonical(addr,4){return Err(crate::errno::EFAULT)}
 crate::usercopy::copy_to_user(addr as usize,&v.to_ne_bytes())
}
fn handle_futex(uaddr:u64,pid:u32){
 let cur=match read_u32(uaddr){Ok(v)=>v,Err(_)=>return};
 if cur&FUTEX_TID_MASK!=pid{return}
 let new=(cur&FUTEX_WAITERS)|FUTEX_OWNER_DIED;
 if write_u32(uaddr,new).is_err(){return}
 /* Linux only wakes when the waiter advertised itself with FUTEX_WAITERS. */
 if cur&FUTEX_WAITERS!=0{let _=crate::futex::wake(uaddr,1);}
}

/* head points at struct robust_list_head { list, futex_offset, list_op_pending }. */
pub fn process_exit(pid:u32,head:u64){
 if head==0||!canonical(head,24){return}
 let offset=match read_u64(head+8){Ok(v)=>v as i64,Err(_)=>return};
 let pending=match read_u64(head+16){Ok(v)=>v,Err(_)=>return};
 if pending!=0{handle_futex((pending as i64).wrapping_add(offset) as u64,pid)}
 let mut entry=match read_u64(head){Ok(v)=>v,Err(_)=>return};
 let mut walked=0;
 while entry!=head&&entry!=0&&walked<MAX_ENTRIES{
  walked+=1;
  handle_futex((entry as i64).wrapping_add(offset) as u64,pid);
  entry=match read_u64(entry){Ok(v)=>v,Err(_)=>return};
 }
}
