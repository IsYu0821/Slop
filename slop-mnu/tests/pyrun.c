#include <stdio.h>
#include <unistd.h>
#include <sys/mount.h>
int main(void){
 if(mkdir("/mntpy",0755)!=0){printf("pyrun: mkdir failed\n");return 1;}
 if(mount("vda","/mntpy","ext4",0,NULL)!=0){printf("pyrun: mount failed\n");return 1;}
 if(chdir("/mntpy")!=0){printf("pyrun: chdir failed\n");return 1;}
 char *argv[]={"/bin/python3","-u","-S","/mntpy/test.py",0};
 char *envp[]={"PYTHONPATH=/mntpy/lib:/mntpy/lib/lib-dynload","PYTHONDONTWRITEBYTECODE=1",0};
 execve("/bin/python3",argv,envp);
 printf("pyrun: exec failed\n");
 return 1;
}
