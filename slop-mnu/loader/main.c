#include "uefi.h"
#define BOOT_MAGIC 0x534c4f504d4e5521ULL
#define BOOT_ABI_VERSION 2
#define EFI_LOADER_DATA 2
#define MAX_EBS_RETRIES 8
struct MemoryRegion { uint64_t base,pages; uint32_t kind,attributes; };
struct Framebuffer { uint64_t base,size;uint32_t width,height,stride,format; };
struct BootInfo { uint64_t magic;uint32_t abi_version,size;struct MemoryRegion* memory_map;UINTN memory_map_len;uint64_t rsdp,initrd_base,initrd_size;struct Framebuffer framebuffer;uint64_t image_base,image_size; };
typedef void (EFIAPI *KernelEntry)(const struct BootInfo*);
int memcmp(const void*,const void*,size_t);
static int guid_eq(const EFI_GUID*a,const EFI_GUID*b){return memcmp(a,b,sizeof(*a))==0;}
static uint64_t find_rsdp(EFI_SYSTEM_TABLE*st){
 static const EFI_GUID acpi20={0x8868e871,0xe4f1,0x11d3,{0xbc,0x22,0x00,0x80,0xc7,0x3c,0x88,0x81}};
 static const EFI_GUID acpi10={0xeb9d2d30,0x2d88,0x11d3,{0x9a,0x16,0x00,0x90,0x27,0x3f,0xc1,0x4d}};
 void*old=0;for(UINTN i=0;i<st->NumberOfTableEntries;i++){EFI_CONFIGURATION_TABLE*c=&st->ConfigurationTable[i];if(guid_eq(&c->VendorGuid,&acpi20))return(uint64_t)c->VendorTable;if(guid_eq(&c->VendorGuid,&acpi10))old=c->VendorTable;}return(uint64_t)old;
}

/* Filled by the PE embedding/link step. */
extern unsigned char __kernel_start[],__kernel_end[]; extern void slop_mnu_kernel_entry(void);
extern unsigned char _binary_build_initramfs_nsar_start[],_binary_build_initramfs_nsar_end[];
const struct BootInfo* g_boot_info;
typedef EFI_STATUS(EFIAPI *EFI_HANDLE_PROTOCOL)(EFI_HANDLE,void*,void**);
struct LoadedImage { uint32_t Revision;uint32_t _pad;EFI_HANDLE ParentHandle;EFI_SYSTEM_TABLE* SystemTable;EFI_HANDLE DeviceHandle;void* FilePath;void* Reserved;uint32_t LoadOptionsSize;uint32_t _pad2;void* LoadOptions;void* ImageBase;uint64_t ImageSize; };
static const EFI_GUID loaded_image_guid={0x5b1b31a1,0x9562,0x11d2,{0x8e,0x3f,0x00,0xa0,0xc9,0x69,0x72,0x3b}};
static uint32_t map_kind(uint32_t t){ return t==7?1:0; } /* EfiConventionalMemory */
EFI_STATUS EFIAPI efi_main(EFI_HANDLE image,EFI_SYSTEM_TABLE* st){
 EFI_BOOT_SERVICES* bs=st->BootServices; UINTN bytes=0,key=0,desc_size=0;uint32_t desc_ver=0;EFI_STATUS s;
 s=bs->GetMemoryMap(&bytes,0,&key,&desc_size,&desc_ver);if(s!=EFI_BUFFER_TOO_SMALL||desc_size<sizeof(EFI_MEMORY_DESCRIPTOR))return s;
 bytes+=desc_size*16;EFI_MEMORY_DESCRIPTOR* raw=0;if(EFI_ERROR(s=bs->AllocatePool(EFI_LOADER_DATA,bytes,(void**)&raw)))return s;
 struct MemoryRegion* cooked=0;UINTN cap=bytes/desc_size;if(EFI_ERROR(s=bs->AllocatePool(EFI_LOADER_DATA,cap*sizeof(*cooked),(void**)&cooked)))return s;
 struct BootInfo* bi=0;if(EFI_ERROR(s=bs->AllocatePool(EFI_LOADER_DATA,sizeof(*bi),(void**)&bi)))return s;
 struct LoadedImage* li=0;if(EFI_ERROR(((EFI_HANDLE_PROTOCOL)bs->HandleProtocol)(image,(void*)&loaded_image_guid,(void**)&li))||!li)return EFI_UNSUPPORTED;
 for(unsigned attempt=0;attempt<MAX_EBS_RETRIES;attempt++){
  UINTN actual=bytes;s=bs->GetMemoryMap(&actual,raw,&key,&desc_size,&desc_ver);if(EFI_ERROR(s))return s;
  UINTN n=actual/desc_size;for(UINTN i=0;i<n;i++){EFI_MEMORY_DESCRIPTOR* d=(void*)((uint8_t*)raw+i*desc_size);cooked[i]=(struct MemoryRegion){d->PhysicalStart,d->NumberOfPages,map_kind(d->Type),(uint32_t)d->Attribute};}
  *bi=(struct BootInfo){BOOT_MAGIC,BOOT_ABI_VERSION,sizeof(*bi),cooked,n,find_rsdp(st),(uint64_t)_binary_build_initramfs_nsar_start,(uint64_t)(_binary_build_initramfs_nsar_end-_binary_build_initramfs_nsar_start),{0},(uint64_t)li->ImageBase,li->ImageSize};
  s=bs->ExitBootServices(image,key);if(s==EFI_SUCCESS){g_boot_info=bi;slop_mnu_kernel_entry();for(;;)__asm__ volatile("cli; hlt");}
 }
 return s;
}

/* Freestanding compiler/runtime primitives used by Rust core and Clang. */
void *memset(void *dst,int value,size_t n){unsigned char*d=dst;for(size_t i=0;i<n;i++)d[i]=(unsigned char)value;return dst;}
void *memcpy(void *dst,const void *src,size_t n){unsigned char*d=dst;const unsigned char*s=src;for(size_t i=0;i<n;i++)d[i]=s[i];return dst;}
void *memmove(void *dst,const void *src,size_t n){unsigned char*d=dst;const unsigned char*s=src;if(d<s){for(size_t i=0;i<n;i++)d[i]=s[i];}else{for(size_t i=n;i;i--)d[i-1]=s[i-1];}return dst;}
void __chkstk(void){}
int memcmp(const void *a,const void *b,size_t n){const unsigned char*x=a,*y=b;for(size_t i=0;i<n;i++){if(x[i]!=y[i])return x[i]<y[i]?-1:1;}return 0;}
