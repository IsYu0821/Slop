obj-m := slotest.o
# Large model: 64-bit absolute data (no 32-bit truncation), PC32 calls stay
# in range because Slop maps modules within 256MB of the kernel image.
# Retpoline thunks conflict with large model; test driver opts out (the
# loader provides its own __x86_return_thunk stub anyway).
ccflags-y := -fno-stack-protector -fno-pic -fno-pie -mcmodel=large -fpatchable-function-entry=0 -mno-indirect-branch-register -mfunction-return=keep
KBUILD_CFLAGS := $(filter-out -mindirect-branch=thunk-extern -mfunction-return=thunk-extern,$(KBUILD_CFLAGS))
