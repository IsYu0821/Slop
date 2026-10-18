#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
python3 - <<'PY'
from pathlib import Path
r=Path('.')
required={
 'loader/entry.S':['sub $8, %rsp','call kmain_bootinfo'],
 'arch/x86_64/syscall.S':['push %r11','push %r15','mov 48(%rsp), %r8','push 56(%rsp)','call syscall_dispatch_raw','EXEC_PENDING','EXEC_RIP','SYSCALL_FRAME','sysretq'],
 'arch/x86_64/exceptions.S':['isr_invalid_tss','isr_stack_segment','isr_alignment_check','call exception_rust'],
 'arch/x86_64/user.S':['enter_user_asm','resume_user_asm','iretq'],
 'kernel/lib.rs':['spawn_user(loaded)','wait_child(user_pid)','scheduler: user process reaped'],
 'kernel/usercopy.rs':['root.query','PRESENT|USER|WRITABLE'],
 'kernel/pipe.rs':['CAPACITY','readq','writeq','EPIPE','wake_all'],
 'tools/acceptance-test.sh':['expected 33','EFAULT passed','posix: fork, pipe, dup2 and wait4 passed','net: udp and tcp echo passed','net-echo.ready','acceptance: PASS'],
 'kernel/console.rs':['THR_EMPTY','TX_EMPTY','pub fn flush()'],
 'kernel/arch/x86_64/cpu.rs':['CPU lacks SYSCALL/SYSRET','CPU lacks NX','cr0|=1<<16'],
 'kernel/acpi.rs':['RSDP missing','MADT missing','IOAPIC missing'],
 'kernel/arch/x86_64/apic.rs':['TIMER_VECTOR','write(0xb0,0)','1<<17'],
 'arch/x86_64/irq.S':['apic_timer_entry','call apic_timer_rust','iretq'],
 'kernel/sync.rs':['IrqSpinLock','irq_save','irq_restore'],
 'kernel/task.rs':['struct Scheduler','spawn_kernel','yield_now','scheduler_self_test','wake_sleepers','apply_arch_state','IA32_FS_BASE','wake_pending','reap_child','fork_process','UserFrame'],
 'kernel/arch/x86_64/msr.rs':['IA32_FS_BASE','IA32_KERNEL_GS_BASE'],
 'kernel/arch/x86_64/descriptors.rs':['set_rsp0'],
 'kernel/arch/x86_64/syscall.rs':['set_kernel_stack'],
 'libc/start.S':['call main','environ','syscall'],
 'libc/syscall.S':['__slop_syscall6','syscall','ret'],
 'libc/string.c':['memcpy','memset','strcmp','strchr'],
 'libc/stdio.c':['printf','puts','emit_uint'],
 'libc/stdlib.c':['malloc','calloc','free','getenv'],
 'libc/unistd.c':['openat','mmap','fork','waitpid','pipe','dup2','_exit'],
 'libc/errno.c':['__errno_location','errno'],
 'tests/hello-c.c':['int main','getenv','c runtime: passed','execve'],
 'tests/hello-posix.c':['pipe(fds)','fork()','dup2(','waitpid(','posix: fork, pipe, dup2 and wait4 passed'],
 'tools/build-libc-test.sh':['libc/start.S','hello-c.c','hello-posix.c','ld.lld','hello-c.elf','hello-posix.elf'],
 'kernel/wait_queue.rs':['struct WaitQueue','pub fn register','wake_one','wake_all'],
 'kernel/vfs.rs':['mount_initramfs','pub fn lookup','pub fn image'],
 'kernel/fd.rs':['pub fn open','pub fn read','pub fn seek','pub fn close','install_pair','clone_incref','OpenKind','dup2'],
 
 'kernel/process.rs':['map_zeroed','map_page','unmap_page','protect_page','BRK_BASE','clone_address_space','teardown_exit','replace_image'],
 'tests/minimal-linux.S':['mov $59,%rax','/bin/hello','argv','envp'],
 'tests/hello-linux.S':['mov $9,%rax','mov $10,%rax','mov $11,%rax','mov $12,%rax','cmpq $1,(%rsp)','/bin/hello-c'],
 'kernel/linux_abi/mod.rs':['sys_execve','copy_exec_vec','load_static_args','commit_exec_return','sys_clone','sys_wait4','sys_pipe','sys_fcntl','sys_getpeername','sockaddr_in_of'],
 'kernel/linux_abi/nr.rs':['CLONE:u64=56','WAIT4:u64=61','DUP2:u64=33','PIPE2:u64=293','GETPEERNAME:u64=52'],
 'kernel/arch/x86_64/port.rs':['pub unsafe fn inb','pub unsafe fn outb','pub unsafe fn inl','pub unsafe fn outl'],
 'kernel/driver/pci.rs':['pub struct Bdf','Cam','read_bars','pub fn count'],
 'kernel/driver/virtio_net.rs':['VIRTIO_VENDOR','VNET_HDR','pub fn poll_frame','pub fn send_frame','AVAIL_OFF'],
 'kernel/net.rs':['OUR_IP','csum_pseudo','arp_lookup','udp_send','tcp_connect','pub fn poll','EHOSTUNREACH'],
 'kernel/socket.rs':['INET_UDP_BASE','INET_TCP_BASE','pub fn inet_kind'],
 'tools/run-qemu.sh':['virtio-net-pci','disable-modern=on','-netdev user'],
 'tools/net-echo.py':['udp_loop','tcp_loop','net-echo.ready'],
 'tools/build-initramfs.py':['/bin/hello-socket','/bin/hello-net'],
 'arch/x86_64/context.S':['switch_context','mov %rsp,(%rdi)','mov %rsi,%rsp'],
}
for name,tokens in required.items():
    text=(r/name).read_text()
    for token in tokens:
        assert token in text,(name,token)
assert (r/'Cargo.toml').read_text().count('version = "0.27.0"')==1
print('source consistency checks passed')
PY
