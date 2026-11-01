#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>
static int send_fd(int sock, int fd){
 char dummy='X';
 struct iovec iov={.iov_base=&dummy,.iov_len=1};
 char cbuf[CMSG_SPACE(sizeof(int))];
 struct msghdr m={0};
 m.msg_iov=&iov; m.msg_iovlen=1;
 m.msg_control=cbuf; m.msg_controllen=sizeof cbuf;
 struct cmsghdr *c=CMSG_FIRSTHDR(&m);
 c->cmsg_level=SOL_SOCKET; c->cmsg_type=SCM_RIGHTS; c->cmsg_len=CMSG_LEN(sizeof(int));
 memcpy(CMSG_DATA(c),&fd,sizeof(int));
 if(sendmsg(sock,&m,0)!=(ssize_t)1){printf("SCM_FAIL sendmsg %d\n",errno);return -1;}
 return 0;
}
static int recv_fd(int sock){
 char dummy=0;
 struct iovec iov={.iov_base=&dummy,.iov_len=1};
 char cbuf[CMSG_SPACE(sizeof(int))];
 struct msghdr m={0};
 m.msg_iov=&iov; m.msg_iovlen=1;
 m.msg_control=cbuf; m.msg_controllen=sizeof cbuf;
 if(recvmsg(sock,&m,0)!=(ssize_t)1){printf("SCM_FAIL recvmsg %d\n",errno);return -1;}
 struct cmsghdr *c=CMSG_FIRSTHDR(&m);
 if(!c||c->cmsg_level!=SOL_SOCKET||c->cmsg_type!=SCM_RIGHTS){puts("SCM_FAIL nocmsg");return -1;}
 int fd=-1; memcpy(&fd,CMSG_DATA(c),sizeof(int));
 return fd;
}
int main(void){
 int sp[2];
 if(socketpair(AF_UNIX,SOCK_STREAM,0,sp)){puts("SCM_FAIL pair");return 1;}
 /* pass the read end of a pipe; write on the other end, read via received fd */
 int pp[2]; if(pipe(pp)){puts("SCM_FAIL pipe");return 1;}
 if(send_fd(sp[0],pp[0]))return 1;
 int got=recv_fd(sp[1]);
 if(got<0)return 1;
 if(write(pp[1],"fdpass",6)!=6){puts("SCM_FAIL write");return 1;}
 char b[8]={0};
 if(read(got,b,6)!=6||memcmp(b,"fdpass",6)){printf("SCM_FAIL read got=%d\n",got);return 1;}
 /* the received fd must be independent: close original, still readable */
 close(pp[0]);
 if(write(pp[1],"again",5)!=5){puts("SCM_FAIL write2");return 1;}
 memset(b,0,sizeof b);
 if(read(got,b,5)!=5||memcmp(b,"again",5)){puts("SCM_FAIL read2");return 1;}
 /* credentials: enable passcred, send empty, check pid */
 int one=1;
 if(setsockopt(sp[1],SOL_SOCKET,SO_PASSCRED,&one,sizeof one)){printf("SCM_FAIL passcred %d\n",errno);return 1;}
 char d='Y'; struct iovec iv={.iov_base=&d,.iov_len=1};
 struct msghdr sm={0}; sm.msg_iov=&iv; sm.msg_iovlen=1;
 if(sendmsg(sp[0],&sm,0)!=(ssize_t)1){puts("SCM_FAIL credsend");return 1;}
 char cbuf[CMSG_SPACE(sizeof(struct ucred))];
 char dd=0; struct iovec iv2={.iov_base=&dd,.iov_len=1};
 struct msghdr rm={0}; rm.msg_iov=&iv2; rm.msg_iovlen=1;
 rm.msg_control=cbuf; rm.msg_controllen=sizeof cbuf;
 if(recvmsg(sp[1],&rm,0)!=(ssize_t)1){puts("SCM_FAIL credrecv");return 1;}
 struct cmsghdr *c=CMSG_FIRSTHDR(&rm);
 if(!c||c->cmsg_type!=SCM_CREDENTIALS){puts("SCM_FAIL nocred");return 1;}
 struct ucred *u=(struct ucred*)CMSG_DATA(c);
 if(u->pid!=getpid()){printf("SCM_FAIL pid %d vs %d\n",u->pid,getpid());return 1;}
 puts("SCM_OK");
 return 0;
}
