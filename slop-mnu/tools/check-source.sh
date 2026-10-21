#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
python3 - <<'PY'
from pathlib import Path
r=Path('.')
required={
 'loader/entry.S':['sub $8, %rsp','call kmain_bootinfo'],
 'arch/x86_64/syscall.S':['push %r11','push %r15','mov 48(%rsp), %r8','push 56(%rsp)','call syscall_dispatch_raw','EXEC_PENDING','EXEC_RIP','SYSCALL_FRAME','iretq','call signal_pending_check','USER_CS','USER_SS'],
 'arch/x86_64/exceptions.S':['isr_invalid_tss','isr_stack_segment','isr_alignment_check','call exception_rust'],
 'arch/x86_64/user.S':['enter_user_asm','resume_user_asm','iretq'],
 'kernel/lib.rs':['spawn_user(loaded)','wait_child(user_pid)','scheduler: user process reaped'],
 'kernel/usercopy.rs':['root.query','PRESENT|USER|WRITABLE'],
 'kernel/pipe.rs':['CAPACITY','readq','writeq','EPIPE','wake_all'],
 'tools/acceptance-test.sh':['expected 33','EFAULT passed','posix: fork, pipe, dup2 and wait4 passed','signals: handler, mask, ignore, default and stop/continue passed','acceptance: PASS'],
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
 'libc/unistd.c':['openat','mmap','fork','waitpid','pipe','dup2','_exit','nanosleep'],
 'libc/errno.c':['__errno_location','errno'],
 'libc/signal.c':['sigaction','sigprocmask','k_sigaction','__slop_sigreturn_trampoline','SA_RESTORER'],
 'libc/sigreturn.S':['__slop_sigreturn_trampoline','mov $15, %eax','syscall'],
 'libc/include/signal.h':['struct sigaction','siginfo_t','ucontext_t','REG_RIP','SA_SIGINFO'],
 'tests/hello-c.c':['int main','getenv','c runtime: passed','execve'],
 'tests/hello-posix.c':['pipe(fds)','fork()','dup2(','waitpid(','posix: fork, pipe, dup2 and wait4 passed','/bin/hello-signal'],
 'tests/hello-signal.c':['sigaction(SIGUSR1','sigprocmask','sigpending','raise(SIGKILL)','raise(SIGSTOP)','SIGCONT','signals: handler, mask, ignore, default and stop/continue passed'],
 'tools/build-libc-test.sh':['libc/start.S','hello-c.c','hello-posix.c','hello-signal.c','libc/signal.c','libc/sigreturn.S','ld.lld','hello-c.elf','hello-posix.elf','hello-signal.elf'],
 'kernel/wait_queue.rs':['struct WaitQueue','pub fn register','wake_one','wake_all'],
 'kernel/vfs.rs':['mount_initramfs','pub fn lookup','pub fn image'],
 'kernel/fd.rs':['pub fn open','pub fn read','pub fn seek','pub fn close','install_pair','clone_incref','OpenKind','dup2'],
 'tools/build-initramfs.py':['NSAR0001','/init','/etc/motd','/bin/hello-c','/bin/hello-posix'],
 'kernel/process.rs':['map_zeroed','map_page','unmap_page','protect_page','BRK_BASE','clone_address_space','teardown_exit','replace_image'],
 'tests/minimal-linux.S':['mov $59,%rax','/bin/hello','argv','envp'],
 'tests/hello-linux.S':['mov $9,%rax','mov $10,%rax','mov $11,%rax','mov $12,%rax','cmpq $1,(%rsp)','/bin/hello-c'],
 'kernel/linux_abi/mod.rs':['sys_execve','copy_exec_vec','load_static_args','commit_exec_return','sys_clone','sys_wait4','sys_pipe','sys_fcntl','sys_rt_sigaction','sys_rt_sigreturn','sys_rt_sigpending','EINTR'],
 'kernel/linux_abi/nr.rs':['CLONE:u64=56','WAIT4:u64=61','DUP2:u64=33','PIPE2:u64=293','RT_SIGPENDING:u64=127'],
 'kernel/signal.rs':['struct Action','prepare_delivery','build_frame','restore_frame','SA_RESTORER','SA_NODEFER','SA_RESETHAND','SIG_IGN','SI_USER'],
 'kernel/task.rs':['stop_current','continue_pid','exit_with_signal','signal_targets','with_process_by_pid'],
 'arch/x86_64/context.S':['switch_context','mov %rsp,(%rdi)','mov %rsi,%rsp'],
 'tools/check-abi.py':['k_sigaction','sigcontext','ucontext','siginfo','REG_%s','ABI layout checks passed'],
 'tools/make-snapshot.py':['SOURCE_SNAPSHOT.txt','FILE MANIFEST','SHA-256','TOTAL FILES'],
}
for name,tokens in required.items():
    text=(r/name).read_text()
    for token in tokens:
        assert token in text,(name,token)
assert (r/'Cargo.toml').read_text().count('version = "0.29.0"')==1
print('source consistency checks passed')
PY
python3 tools/check-abi.py
