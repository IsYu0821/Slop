#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
struct input_event { long sec, usec; unsigned short type, code; int value; };
int main(void){
 int f=-1;
 for(int i=0;i<50&&f<0;i++){ f=open("/dev/input/event1",O_RDONLY|O_NONBLOCK); if(f<0) usleep(200000); }
 if(f<0){printf("MOUSE_FAIL open %d\n",errno);return 1;}
 close(f);
 f=open("/dev/input/event1",O_RDONLY);
 if(f<0){printf("MOUSE_FAIL open2 %d\n",errno);return 1;}
 printf("MOUSE_READY\n");
 struct input_event ev[16];
 int got_rel=0, got_btn=0;
 for(int iter=0;iter<30;iter++){
  int n=read(f,ev,sizeof ev);
  if(n<0){ if(errno==11){usleep(100000);continue;} printf("MOUSE_FAIL read %d\n",errno); return 1; }
  for(int i=0;i+24<=n;i+=24){
   unsigned t=ev[i/24].type, c=ev[i/24].code; int v=ev[i/24].value;
   printf("MOUSE_EV t=%u c=%u v=%d\n",t,c,v);
   if(t==2&&(c==0||c==1)) got_rel=1;
   if(t==1&&c>=0x110&&c<=0x112) got_btn=1;
  }
  if(got_rel&&got_btn) break;
 }
 if(!got_rel){puts("MOUSE_FAIL no rel");return 1;}
 if(!got_btn){puts("MOUSE_FAIL no btn");return 1;}
 puts("MOUSE_OK");
 return 0;
}
