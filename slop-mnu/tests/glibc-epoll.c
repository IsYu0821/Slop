#define _GNU_SOURCE
#include <stdio.h>
#include <unistd.h>
#include <string.h>
#include <errno.h>
#include <signal.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <sys/signalfd.h>
int main(void){
 int ep=epoll_create1(EPOLL_CLOEXEC);
 if(ep<0){printf("EPOLL_FAIL create %d\n",errno);return 1;}
 int pp[2]; if(pipe(pp)){puts("EPOLL_FAIL pipe");return 1;}
 int efd=eventfd(0,EFD_CLOEXEC|EFD_NONBLOCK);
 if(efd<0){printf("EPOLL_FAIL eventfd %d\n",errno);return 1;}
 sigset_t m; sigemptyset(&m); sigaddset(&m,SIGUSR1);
 if(sigprocmask(SIG_BLOCK,&m,0)){puts("EPOLL_FAIL sigmask");return 1;}
 int sfd=signalfd(-1,&m,SFD_CLOEXEC|SFD_NONBLOCK);
 if(sfd<0){printf("EPOLL_FAIL signalfd %d\n",errno);return 1;}
 struct epoll_event ev;
 ev.events=EPOLLIN; ev.data.fd=pp[0];
 if(epoll_ctl(ep,EPOLL_CTL_ADD,pp[0],&ev)){puts("EPOLL_FAIL ctl pipe");return 1;}
 ev.events=EPOLLIN; ev.data.fd=efd;
 if(epoll_ctl(ep,EPOLL_CTL_ADD,efd,&ev)){puts("EPOLL_FAIL ctl efd");return 1;}
 ev.events=EPOLLIN; ev.data.fd=sfd;
 if(epoll_ctl(ep,EPOLL_CTL_ADD,sfd,&ev)){puts("EPOLL_FAIL ctl sfd");return 1;}
 /* 1. pipe readable */
 if(write(pp[1],"hi",2)!=2){puts("EPOLL_FAIL write");return 1;}
 struct epoll_event out[4];
 int n=epoll_wait(ep,out,4,2000);
 if(n!=1||out[0].data.fd!=pp[0]){printf("EPOLL_FAIL wait1 n=%d\n",n);return 1;}
 char b[4]={0}; if(read(pp[0],b,2)!=2||memcmp(b,"hi",2)){puts("EPOLL_FAIL read");return 1;}
 /* 2. eventfd readable */
 uint64_t one=1; if(write(efd,&one,8)!=8){puts("EPOLL_FAIL efdwrite");return 1;}
 n=epoll_wait(ep,out,4,2000);
 if(n!=1||out[0].data.fd!=efd){printf("EPOLL_FAIL wait2 n=%d\n",n);return 1;}
 uint64_t v=0; if(read(efd,&v,8)!=8||v!=1){puts("EPOLL_FAIL efdread");return 1;}
 /* 3. signalfd readable (raise to self) */
 if(kill(getpid(),SIGUSR1)){puts("EPOLL_FAIL kill");return 1;}
 n=epoll_wait(ep,out,4,2000);
 if(n!=1||out[0].data.fd!=sfd){printf("EPOLL_FAIL wait3 n=%d\n",n);return 1;}
 struct signalfd_siginfo si; if(read(sfd,&si,sizeof si)!=(ssize_t)sizeof si){puts("EPOLL_FAIL sfdread");return 1;}
 if(si.ssi_signo!=SIGUSR1){puts("EPOLL_FAIL signo");return 1;}
 /* 4. timeout with nothing ready */
 n=epoll_wait(ep,out,4,100);
 if(n!=0){printf("EPOLL_FAIL wait4 n=%d\n",n);return 1;}
 /* 5. poll() on pipe (now empty -> 0) then writable check */
 struct pollfd {int fd; short events; short revents;} pf;
 pf.fd=pp[1]; pf.events=0x4; pf.revents=0; /* POLLOUT */
 /* use libc poll via syscall number to avoid header dep */
 extern int poll(void*,unsigned long,int);
 if(poll(&pf,1,500)!=1||!(pf.revents&0x4)){puts("EPOLL_FAIL poll");return 1;}
 puts("EPOLL_OK");
 return 0;
}
