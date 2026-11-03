#include <stdio.h>
#include <unistd.h>
#include <sys/mount.h>
#include <sys/stat.h>
int main(void){
 mkdir("/mntpy",0755);
 /* Try ext4 first, then ext2, so the same harness runs on either image. */
 if(mount("vda","/mntpy","ext4",0,NULL)!=0){
  if(mount("vda","/mntpy","ext2",0,NULL)!=0){printf("pyrun: mount failed\n");return 1;}
 }
 if(chdir("/mntpy")!=0){printf("pyrun: chdir failed\n");return 1;}
 char *argv[]={"/bin/python3","-u","-S","/mntpy/test.py",0};
 char *envp[]={"PYTHONPATH=/mntpy/lib:/mntpy/lib/lib-dynload","PYTHONDONTWRITEBYTECODE=1",0};
 execve("/bin/python3",argv,envp);
 printf("pyrun: exec failed\n");
 return 1;
}
