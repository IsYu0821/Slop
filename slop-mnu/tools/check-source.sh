#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
python3 - <<'PY'
from pathlib import Path
import re
r=Path('.')
required={
 'loader/entry.S':['sub $8, %rsp','call kmain_bootinfo'],
 'arch/x86_64/syscall.S':['push %r11','push %r15','mov 48(%rsp), %r8','push 56(%rsp)','call syscall_dispatch_raw','%gs:73','%gs:40','%gs:24','iretq','signal_pending_check','USER_CS','USER_SS'],
 'arch/x86_64/exceptions.S':['isr_invalid_tss','isr_stack_segment','isr_alignment_check','call exception_rust','movups 0(%rsp),%xmm0'],
 'arch/x86_64/user.S':['enter_user_asm','resume_user_asm','iretq'],
 'kernel/lib.rs':['spawn_user(loaded)','wait_child(user_pid)','scheduler: user process reaped','kmain_high_continue','apply_relocations','keep_slots','KIMAGE_BASE'],
 'kernel/usercopy.rs':['root.query','PRESENT|USER|WRITABLE'],
 'kernel/pipe.rs':['CAPACITY','readq','writeq','EPIPE','wake_all'],
 'tools/acceptance-test.sh':['expected 33','EFAULT passed','posix: fork, pipe, dup2 and wait4 passed','net: udp and tcp echo passed','net-echo.ready','acceptance: PASS'],
 'kernel/console.rs':['THR_EMPTY','TX_EMPTY','pub fn flush()'],
 'kernel/arch/x86_64/cpu.rs':['CPU lacks SYSCALL/SYSRET','CPU lacks NX','cr0|=1<<16'],
 'kernel/acpi.rs':['RSDP missing','MADT missing','IOAPIC missing'],
 'kernel/boot.rs':['apply_relocations','base relocations stripped','unsupported relocation type'],
 'kernel/arch/x86_64/phys.rs':['PHYS_BASE','KIMAGE_BASE','phys_to_virt','virt_to_phys','pub struct EarlyPhys'],
 'kernel/arch/x86_64/paging.rs':['install_direct_map','keep_slots','enter_high','clone_bootstrap'],
 'loader/main.c':['loaded_image_guid','image_base','li->ImageSize'],
 'tests/minimal-linux.ld':['0x0000000000400000'],
 'tests/nopie-helper.c':['nopie-helper ran','return 17'],
 'tools/differential-test.sh':['nopie-helper','DIFF_NOPIE_OK','DIFF_STOP_OK','DIFF_FP_OK','DIFF_SIGNALFD_STOP_OK','DIFF_SIGNALFD_NOCLDSTOP_OK','DIFF_CFR_OK','DIFF_SPLICE_OK','DIFF_TEE_OK','DIFF_FALLOC_OK','DIFF_EXT2_OK','build-ext2','virtio-blk-pci,drive=hd0','e2fsck'],
 'kernel/arch/x86_64/apic.rs':['TIMER_VECTOR','write(0xb0,0)','1<<17','phys_to_virt'],
 'arch/x86_64/irq.S':['apic_timer_entry','mov 384(%r15), %rdi','call apic_timer_rust','iretq','movups 0(%rsp),%xmm0'],
 'kernel/arch/x86_64/tsc.rs':['pub fn rdtsc','lfence','rdtsc'],
 'kernel/arch/x86_64/pit.rs':['pub const INPUT_HZ','pub fn measure_tsc','pub fn tsc_hz'],
 'kernel/vdso.rs':['pub const VDSO_BASE','pub const VVAR_BASE','pub fn map_into','pub fn update','__vdso_clock_gettime'],
 'vdso/vdso.S':['__vdso_clock_gettime','__vdso_gettimeofday','__vdso_time','__vdso_getcpu'],
 'vdso/vdso.ld':['ENTRY(__vdso_clock_gettime)'],
 'tools/build-vdso.sh':['vdso/vdso.S','vdso/vdso.ld','--hash-style=both','__vdso_clock_gettime'],
 'kernel/sync.rs':['IrqSpinLock','irq_save','irq_restore'],
'kernel/smp.rs':['ap_main','TRAMPOLINE_ADDR','CPU_READY','wait_ready','send_sipi','send_init','fill_gdt','load_tr','cpu_list','smp: single CPU','smp: self-test passed'],
'arch/x86_64/trampoline.S':['trampoline_start','trampoline_end','TRAMP_BASE','ljmp','lgdt','hlt'],
 'kernel/task.rs':['fpu_area','fpu_save_regs','fpu_restore_regs','CURRENT_FPU','Stopped','stop_current','cont_pending','store_sigchld','sch_code','wake_parent_for_wait','struct Scheduler','spawn_kernel','yield_now','scheduler_self_test','wake_sleepers','apply_arch_state','IA32_FS_BASE','wake_pending','reap_child','reap_child_opts','fork_process','UserFrame','account_tick','signal_task','current_tgid','take_slot'],
 'kernel/arch/x86_64/msr.rs':['IA32_FS_BASE','IA32_KERNEL_GS_BASE'],
 'kernel/signal.rs':['prepare_delivery','build_frame','restore_frame','UContext','SigInfo','MachineContext','REG_RIP','frame_size','default_stops','default_terminates','stop_current','fpregs','CLD_STOPPED','CLD_CONTINUED','CLD_EXITED','CLD_KILLED'],
 'libc/include/signal.h':['ucontext_t','mcontext_t','gregset_t','siginfo_t','kernel_sigaction','SA_SIGINFO'],
 'libc/sigreturn.S':['__slop_sigreturn_trampoline','rt_sigreturn'],
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
 'kernel/vfs.rs':['mount_initramfs','pub fn lookup','pub fn image','MAX_FILE_PAGES','ensure_page_of','free_beyond','page_frame_of'],
 'kernel/vm.rs':['pub fn replace','pub fn insert','pub fn allocate','pub fn remove'],
 'kernel/fd.rs':['pub fn open','pub fn read','pub fn seek','pub fn close','install_pair','clone_incref','OpenKind','dup2','signalfd_read','signalfd_readable','signalfd_set_mask','any_open_ino'],
 
 'kernel/process.rs':['share_slot','release_frame','map_page','unmap_page','protect_page','BRK_BASE','clone_address_space','teardown_exit','replace_image','utime_ns'],
 'kernel/futex.rs':['fn wait','pub fn wake','pub fn dispatch','block_until'],
  'tests/differential.c':['DIFF_PTHREAD_OK','DIFF_COND_OK','DIFF_KILL_OK','DIFF_TLS_OK','DIFF_STOP_OK','DIFF_FP_OK','DIFF_SIGNALFD_STOP_OK','DIFF_SIGNALFD_NOCLDSTOP_OK','DIFF_CFR_OK','DIFF_SPLICE_OK','DIFF_TEE_OK','DIFF_FALLOC_OK','DIFF_TCPSRV_OK','DIFF_UDPLOOP_OK','DIFF_UNIXDG_OK','DIFF_AT_OK','DIFF_BIGFILE_OK','DIFF_EXT2_OK','DIFF_NOPIE_OK'],
 'tests/minimal-linux.S':['mov $59,%rax','/bin/hello','argv','envp'],
 'tests/hello-linux.S':['mov $9,%rax','mov $10,%rax','mov $11,%rax','mov $12,%rax','cmpq $1,(%rsp)','/bin/hello-c'],
  'kernel/linux_abi/mod.rs':['sys_rt_sigsuspend','sys_execve','copy_exec_vec','load_static_args','commit_exec_return','sys_clone','sys_wait4','sys_pipe','sys_fcntl','sys_getpeername','sockaddr_in_of','sys_getrusage','rusage_buf','sys_tgkill','sys_signalfd4','signalfd_readable','signalfd_set_mask','sys_rt_sigtimedwait','dgram_sendto','write_sockaddr_un','sys_times','at_resolve','sys_renameat_flags','sys_execve_resolved'],
 'kernel/linux_abi/nr.rs':['CLONE:u64=56','WAIT4:u64=61','DUP2:u64=33','PIPE2:u64=293','GETPEERNAME:u64=52'],
 'kernel/arch/x86_64/port.rs':['pub unsafe fn inb','pub unsafe fn outb','pub unsafe fn inl','pub unsafe fn outl'],
 'kernel/driver/pci.rs':['pub struct Bdf','Cam','read_bars','pub fn count'],
 'kernel/driver/virtio_net.rs':['pub fn poll_frame','pub fn send_frame','virt_to_phys','MODERN_NET'],
 'kernel/driver/virtio.rs':['MODERN_BLK','MODERN_NET','modern_caps','negotiate_modern','queue_setup','notify_base','VIRTIO_F_VERSION_1'],
 'kernel/net.rs':['OUR_IP','csum_pseudo','arp_lookup','udp_send','tcp_connect','tcp_listen','tcp_accept','is_loopback','LOOPBACK','SynRcvd','pub fn poll','EHOSTUNREACH'],
 'kernel/socket.rs':['INET_UDP_BASE','INET_TCP_BASE','pub fn inet_kind','close_by_id'],
 'kernel/ext2.rs':['create_file','write_file','truncate_file','unlink_path','mkdir_path','rmdir_path','dir_insert','alloc_block','free_block','patch_inode','inode_sector_count','link_path','rename_path','symlink_path','readlink_into','free_all_blocks'],
 'kernel/ext4.rs':['extent_lookup','EXTENTS_FL','EXT_MAGIC','readlink_into','INO_BASE','is_ext4_ino','probe'],
 'tools/run-qemu.sh':['virtio-net-pci','-netdev user'],
 'tools/net-echo.py':['udp_loop','tcp_loop','net-echo.ready'],
 'tools/build-initramfs.py':['/bin/hello-socket','/bin/hello-net','/lib/vdso.so'],
 'arch/x86_64/context.S':['switch_context','mov %rsp,(%rdi)','mov %rsi,%rsp','movups %xmm0','fpu_restore_regs'],
 'arch/x86_64/syscall.S':['push %r11','push %r15','mov 48(%rsp), %r8','push 56(%rsp)','call syscall_dispatch_raw','%gs:73','%gs:40','%gs:24','iretq','signal_pending_check','USER_CS','USER_SS','%gs:0'],
 'tests/bbtest.sh':['out shell-echo','out fs-cat','out proc-signal','BB|done|end'],
 'tools/busybox-test.sh':['busybox compat: PASS','build/busybox','tests/musl/busybox'],
 'arch/x86_64/user.S':['enter_user_asm','resume_user_asm','iretq'],
}
for name,tokens in required.items():
    text=(r/name).read_text()
    for token in tokens:
        assert token in text,(name,token)
assert (r/'Cargo.toml').read_text().count('version = "%s"'%re.search(r'^## (\S+) - ',(r/'CHANGELOG.md').read_text(),re.M).group(1))==1
print('source consistency checks passed')
PY
