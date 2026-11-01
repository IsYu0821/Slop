#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>
int main(void){
 int sp[2];
 if(socketpair(AF_UNIX,SOCK_DGRAM,0,sp)){puts("DGRAM_FAIL pair");return 1;}
 /* pass two fds at once over datagram */
 int p1[2],p2[2];
 if(pipe(p1)||pipe(p2)){puts("DGRAM_FAIL pipe");return 1;}
 char dummy='D';
 struct iovec iov={.iov_base=&dummy,.iov_len=1};
 char cbuf[CMSG_SPACE(2*sizeof(int))];
 struct msghdr m={0};
 m.msg_iov=&iov; m.msg_iovlen=1;
 m.msg_control=cbuf; m.msg_controllen=sizeof cbuf;
 struct cmsghdr *c=CMSG_FIRSTHDR(&m);
 c->cmsg_level=SOL_SOCKET; c->cmsg_type=SCM_RIGHTS; c->cmsg_len=CMSG_LEN(2*sizeof(int));
 int fds[2]={p1[0],p2[0]}; memcpy(CMSG_DATA(c),fds,sizeof fds);
 m.msg_controllen=CMSG_SPACE(2*sizeof(int));
 if(sendmsg(sp[0],&m,0)!=(ssize_t)1){printf("DGRAM_FAIL send %d\n",errno);return 1;}
 char dd=0; struct iovec iv={.iov_base=&dd,.iov_len=1};
 char rc[CMSG_SPACE(2*sizeof(int))];
 struct msghdr rm={0}; rm.msg_iov=&iv; rm.msg_iovlen=1;
 rm.msg_control=rc; rm.msg_controllen=sizeof rc;
 if(recvmsg(sp[1],&rm,0)!=(ssize_t)1){puts("DGRAM_FAIL recv");return 1;}
 c=CMSG_FIRSTHDR(&rm);
 if(!c||c->cmsg_type!=SCM_RIGHTS){puts("DGRAM_FAIL nocmsg");return 1;}
 int n=(c->cmsg_len-CMSG_LEN(0))/sizeof(int);
 if(n!=2){printf("DGRAM_FAIL n=%d\n",n);return 1;}
 int g1,g2; memcpy(&g1,CMSG_DATA(c),4); memcpy(&g2,(char*)CMSG_DATA(c)+4,4);
 if(write(p1[1],"one",3)!=3||write(p2[1],"two",3)!=3){puts("DGRAM_FAIL write");return 1;}
 char b[4]={0};
 if(read(g1,b,3)!=3||memcmp(b,"one",3)){puts("DGRAM_FAIL read1");return 1;}
 memset(b,0,sizeof b);
 if(read(g2,b,3)!=3||memcmp(b,"two",3)){puts("DGRAM_FAIL read2");return 1;}
 puts("DGRAM_OK");
 return 0;
}
