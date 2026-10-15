use core::{cell::UnsafeCell, hint::spin_loop, ops::{Deref, DerefMut}, sync::atomic::{AtomicBool, Ordering}};

pub struct SpinLock<T> { held: AtomicBool, value: UnsafeCell<T> }
unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}
impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self { Self { held: AtomicBool::new(false), value: UnsafeCell::new(value) } }
    pub fn lock(&self) -> Guard<'_, T> {
        while self.held.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            while self.held.load(Ordering::Relaxed) { spin_loop(); }
        }
        Guard { lock: self }
    }
}
pub struct Guard<'a, T> { lock: &'a SpinLock<T> }
impl<T> Deref for Guard<'_, T> { type Target=T; fn deref(&self)->&T { unsafe { &*self.lock.value.get() } } }
impl<T> DerefMut for Guard<'_, T> { fn deref_mut(&mut self)->&mut T { unsafe { &mut *self.lock.value.get() } } }
impl<T> Drop for Guard<'_, T> { fn drop(&mut self) { self.lock.held.store(false, Ordering::Release); } }

/// Spin lock that cannot be interrupted on the local CPU while held.
pub struct IrqSpinLock<T>{inner:SpinLock<T>}
impl<T> IrqSpinLock<T>{pub const fn new(value:T)->Self{Self{inner:SpinLock::new(value)}}pub fn lock(&self)->IrqGuard<'_,T>{let flags=irq_save();IrqGuard{guard:core::mem::ManuallyDrop::new(self.inner.lock()),flags}}}
pub struct IrqGuard<'a,T>{guard:core::mem::ManuallyDrop<Guard<'a,T>>,flags:u64}
impl<T>Deref for IrqGuard<'_,T>{type Target=T;fn deref(&self)->&T{&self.guard}}
impl<T>DerefMut for IrqGuard<'_,T>{fn deref_mut(&mut self)->&mut T{&mut self.guard}}
impl<T>Drop for IrqGuard<'_,T>{fn drop(&mut self){unsafe{core::mem::ManuallyDrop::drop(&mut self.guard)}irq_restore(self.flags)}}
#[inline]fn irq_save()->u64{let flags:u64;unsafe{core::arch::asm!("pushfq","pop {}","cli",out(reg)flags,options(nomem));}flags}
#[inline]fn irq_restore(flags:u64){if flags&(1<<9)!=0{unsafe{core::arch::asm!("sti",options(nomem,nostack,preserves_flags));}}}
