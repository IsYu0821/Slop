#ifndef SLOP_MNU_UEFI_H
#define SLOP_MNU_UEFI_H
#include <stdint.h>
#include <stddef.h>
#define EFIAPI __attribute__((ms_abi))
typedef uint64_t EFI_STATUS; typedef void *EFI_HANDLE; typedef uint16_t CHAR16; typedef uint64_t UINTN;
#define EFI_SUCCESS 0
#define EFI_BUFFER_TOO_SMALL 0x8000000000000005ULL
#define EFI_ERROR(x) (((x)>>63)!=0)
typedef struct { uint32_t Type; uint32_t Pad; uint64_t PhysicalStart,VirtualStart,NumberOfPages,Attribute; } EFI_MEMORY_DESCRIPTOR;
typedef struct { uint64_t Signature;uint32_t Revision,HeaderSize,Crc32,Reserved; } EFI_TABLE_HEADER;
typedef EFI_STATUS(EFIAPI *EFI_GET_MEMORY_MAP)(UINTN*,EFI_MEMORY_DESCRIPTOR*,UINTN*,UINTN*,uint32_t*);
typedef EFI_STATUS(EFIAPI *EFI_ALLOCATE_POOL)(uint32_t,UINTN,void**);
typedef EFI_STATUS(EFIAPI *EFI_EXIT_BOOT_SERVICES)(EFI_HANDLE,UINTN);
typedef struct { uint32_t Data1;uint16_t Data2,Data3;uint8_t Data4[8]; } EFI_GUID;
typedef struct { EFI_GUID VendorGuid;void* VendorTable; } EFI_CONFIGURATION_TABLE;
typedef struct { EFI_TABLE_HEADER Hdr; void* RaiseTPL;void* RestoreTPL;void* AllocatePages;void* FreePages;EFI_GET_MEMORY_MAP GetMemoryMap;EFI_ALLOCATE_POOL AllocatePool;void* FreePool;void* CreateEvent;void* SetTimer;void* WaitForEvent;void* SignalEvent;void* CloseEvent;void* CheckEvent;void* InstallProtocolInterface;void* ReinstallProtocolInterface;void* UninstallProtocolInterface;void* HandleProtocol;void* Reserved;void* RegisterProtocolNotify;void* LocateHandle;void* LocateDevicePath;void* InstallConfigurationTable;void* LoadImage;void* StartImage;void* Exit;void* UnloadImage;EFI_EXIT_BOOT_SERVICES ExitBootServices; } EFI_BOOT_SERVICES;
typedef struct { uint32_t Data1;uint16_t Data2,Data3;uint8_t Data4[8]; } EFI_GUID;
typedef struct { EFI_GUID VendorGuid;void* VendorTable; } EFI_CONFIGURATION_TABLE;
typedef struct { EFI_TABLE_HEADER Hdr;CHAR16* FirmwareVendor;uint32_t FirmwareRevision;EFI_HANDLE ConsoleInHandle;void* ConIn;EFI_HANDLE ConsoleOutHandle;void* ConOut;EFI_HANDLE StandardErrorHandle;void* StdErr;void* RuntimeServices;EFI_BOOT_SERVICES* BootServices;UINTN NumberOfTableEntries;EFI_CONFIGURATION_TABLE* ConfigurationTable; } EFI_SYSTEM_TABLE;
#endif
