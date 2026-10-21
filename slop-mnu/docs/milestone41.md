# Milestone 41: Block devices and virtio-blk

Added a virtio-blk legacy driver, `/dev/vda` devtmpfs node, block device file
operations with offset tracking, `read`, `write`, `lseek`, and `fstat` support.
Disk I/O is performed synchronously through descriptor chains.

Acceptance suite remains green.
