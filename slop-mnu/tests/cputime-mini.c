#define _GNU_SOURCE
#include <stdio.h>
#include <time.h>
#include <sys/resource.h>
#include <unistd.h>
#include <string.h>
#include <sched.h>
#include <stdlib.h>
static long long ms(struct timespec a, struct timespec b){return (b.tv_sec-a.tv_sec)*1000LL+(b.tv_nsec-a.tv_nsec)/1000000LL;}
int main(int argc, char **argv){
 int pin=-1;
 if(argc>1)pin=atoi(argv[1]);
 if(pin>=0){cpu_set_t s;CPU_ZERO(&s);CPU_SET(pin,&s);
  if(sched_setaffinity(0,sizeof s,&s)!=0){puts("CPUTIME_FAIL affinity");return 1;}}
 int c0=sched_getcpu();
 struct timespec w0,c0t,ct0;
 if(clock_gettime(CLOCK_MONOTONIC,&w0)!=0){puts("CPUTIME_FAIL mono0");return 1;}
 if(clock_gettime(CLOCK_PROCESS_CPUTIME_ID,&c0t)!=0){puts("CPUTIME_FAIL proc0");return 1;}
 if(clock_gettime(CLOCK_THREAD_CPUTIME_ID,&ct0)!=0){puts("CPUTIME_FAIL thr0");return 1;}
 volatile unsigned long spin=0;
 int cpus[8]={0};
 for(;;){struct timespec n;spin++;clock_gettime(CLOCK_MONOTONIC,&n);
  if((spin&1023)==0){int c=sched_getcpu();if(c>=0&&c<8)cpus[c]++;}
  if(ms(w0,n)>=150)break;}
 int c1=sched_getcpu();
 struct timespec w1,c1t,ct1;
 clock_gettime(CLOCK_MONOTONIC,&w1);
 clock_gettime(CLOCK_PROCESS_CPUTIME_ID,&c1t);
 clock_gettime(CLOCK_THREAD_CPUTIME_ID,&ct1);
 long long wms=ms(w0,w1),cms=ms(c0t,c1t),tms=ms(ct0,ct1);
 struct rusage ru;memset(&ru,0,sizeof ru);getrusage(RUSAGE_SELF,&ru);
 long long ru_u=ru.ru_utime.tv_sec*1000LL+ru.ru_utime.tv_usec/1000;
 long long ru_s=ru.ru_stime.tv_sec*1000LL+ru.ru_stime.tv_usec/1000;
 printf("CPUTIME wall=%lld proc=%lld thread=%lld ru_u=%lld ru_s=%lld spin=%lu cpu=%d->%d pin=%d oncpu=[%d,%d,%d,%d]\n",wms,cms,tms,ru_u,ru_s,spin,c0,c1,pin,cpus[0],cpus[1],cpus[2],cpus[3]);
 if(cms<30){puts("CPUTIME_FAIL proc<30");return 1;}
 if(cms>wms+50){puts("CPUTIME_FAIL proc>wall+50");return 1;}
 if(tms<30){puts("CPUTIME_FAIL thread<30");return 1;}
 puts("CPUTIME_OK");
 return 0;
}
