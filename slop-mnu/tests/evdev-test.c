#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <sys/ioctl.h>
#include <unistd.h>
struct input_event { long sec, usec; unsigned short type, code; int value; };
int main(void){
 int dd=open("/dev/input",0x10000); /* O_DIRECTORY */
 printf("EVDEV_NOTE input dir fd=%d err=%d\n",dd,dd<0?errno:0);
 if(dd>=0)close(dd);
 int f=-1;
 for(int i=0;i<50&&f<0;i++){
  f=open("/dev/input/event0",O_RDONLY|O_NONBLOCK);
  if(f<0){usleep(200000);}
 }
 if(f<0){printf("EVDEV_FAIL open %d\n",errno);return 1;}
 /* nonblock with no data yet: expect EAGAIN (if a key was already pressed
    we may get data; either is fine, but EAGAIN proves nonblock works) */
 struct input_event ev[8];
 int n=read(f,ev,sizeof ev);
 if(n<0&&errno!=11){printf("EVDEV_FAIL nbread %d\n",errno);return 1;}
 if(n>0) printf("EVDEV_NOTE early %d bytes\n",n);
 close(f);
 /* blocking read with 3s timeout via alarm-free poll: just read (test
    harness sends a key while we block) */
 f=open("/dev/input/event0",O_RDONLY);
 if(f<0){printf("EVDEV_FAIL open2 %d\n",errno);return 1;}
 /* query version */
 unsigned ver=0;
 if(ioctl(f,0x80044501,&ver)||ver!=0x10001){printf("EVDEV_FAIL ver %x\n",ver);return 1;}
 char name[64]={0};
 int nl=ioctl(f,0x80404506,name);
 if(nl<=0){printf("EVDEV_FAIL name %d\n",nl);return 1;}
 printf("EVDEV_NOTE dev %s\n",name);
 n=read(f,ev,sizeof ev);
 if(n<24){printf("EVDEV_FAIL bread %d\n",n);return 1;}
 int got_a=0;
 for(int i=0;i+24<=n;i+=24){
  unsigned short t=ev[i/24].type, c=ev[i/24].code; int v=ev[i/24].value;
  printf("EVDEV_NOTE ev t=%u c=%u v=%d\n",t,c,v);
  if(t==1&&c==30&&v==1) got_a=1;
 }
 /* drain release + syn */
 n=read(f,ev,sizeof ev);
 for(int i=0;i+24<=n;i+=24) printf("EVDEV_NOTE ev2 t=%u c=%u v=%d\n",ev[i/24].type,ev[i/24].code,ev[i/24].value);
 if(!got_a){puts("EVDEV_FAIL no KEY_A press");return 1;}
 puts("EVDEV_OK");
 return 0;
}
