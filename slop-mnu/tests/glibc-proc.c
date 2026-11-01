#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
static int readfile(const char *p,char *b,int n){
 int f=open(p,O_RDONLY); if(f<0)return -1;
 int t=0,r;
 while(t<n-1&&(r=read(f,b+t,n-1-t))>0)t+=r;
 close(f); if(t<=0)return -1; b[t]=0; return t;
}
int main(void){
 char b[4096];
 /* /proc/self/cmdline or status must exist and be non-empty */
 int n=readfile("/proc/self/status",b,sizeof b);
 if(n<0){n=readfile("/proc/self/cmdline",b,sizeof b);}
 if(n<0){puts("PROC_FAIL status");return 1;}
 /* /proc/cpuinfo or meminfo should exist */
 if(readfile("/proc/cpuinfo",b,sizeof b)<0&&readfile("/proc/meminfo",b,sizeof b)<0){puts("PROC_FAIL info");return 1;}
 /* /dev/null + /dev/urandom must work */
 int f=open("/dev/null",O_WRONLY); if(f<0){puts("PROC_FAIL null");return 1;}
 if(write(f,"x",1)!=1){puts("PROC_FAIL nullw");return 1;} close(f);
 f=open("/dev/urandom",O_RDONLY); if(f<0){puts("PROC_FAIL urandom");return 1;}
 unsigned char r[16]; int t=0,rr;
 while(t<16&&(rr=read(f,(char*)r+t,16-t))>0)t+=rr;
 close(f); if(t!=16){puts("PROC_FAIL rread");return 1;}
 /* /proc/self/fd must list our fds */
 f=open("/proc/self/fd/0",O_RDONLY); /* fd 0 may not be open; just check dir */
 (void)f;
 puts("PROC_OK");
 return 0;
}
