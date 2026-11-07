#include <stdio.h>
#include <unistd.h>
#include <sys/mount.h>
#include <sys/stat.h>
/* Entry for the official-test harness: mount the guest root disk and exec
   the unmodified CPython from it. The FHS layout under /mntpy makes
   unmodified CPython locate its stdlib exe-relative ($exeprefix/lib/
   python3.12), so no PYTHONPATH is needed. */
int main(void){
 mkdir("/mntpy",0755);
 /* Try ext4 first, then ext2, so the same harness runs on either image. */
 if(mount("vda","/mntpy","ext4",0,NULL)!=0){
  if(mount("vda","/mntpy","ext2",0,NULL)!=0){printf("pyrun: mount failed\n");return 1;}
 }
 if(chdir("/mntpy")!=0){printf("pyrun: chdir failed\n");return 1;}
 char *argv[]={"/mntpy/usr/bin/python3","-u","-S","/mntpy/test.py",0};
 char *envp[]={"PYTHONDONTWRITEBYTECODE=1",0};
 execve("/mntpy/usr/bin/python3",argv,envp);
 printf("pyrun: exec failed\n");
 return 1;
}
