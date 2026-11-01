#define _GNU_SOURCE
#include <stdio.h>
#include <pthread.h>
#include <time.h>
#include <errno.h>
static pthread_mutex_t mtx=PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv=PTHREAD_COND_INITIALIZER;
static int ready=0;
static void *worker(void *a){
 struct timespec ts; clock_gettime(CLOCK_REALTIME,&ts); ts.tv_nsec+=200*1000000;
 if(ts.tv_nsec>=1000000000){ts.tv_sec++;ts.tv_nsec-=1000000000;}
 pthread_mutex_lock(&mtx);
 int r=pthread_cond_timedwait(&cv,&mtx,&ts); /* expect ETIMEDOUT */
 if(r!=ETIMEDOUT){printf("TIMED_FAIL first=%d\n",r);return (void*)1;}
 /* second wait: main will signal */
 clock_gettime(CLOCK_REALTIME,&ts); ts.tv_sec+=5;
 while(!ready){r=pthread_cond_timedwait(&cv,&mtx,&ts); if(r!=0&&r!=ETIMEDOUT){printf("TIMED_FAIL second=%d\n",r);return (void*)1;} if(r==ETIMEDOUT)break;}
 pthread_mutex_unlock(&mtx);
 if(!ready){puts("TIMED_FAIL nosignal");return (void*)1;}
 return (void*)0;
}
int main(void){
 pthread_t t; if(pthread_create(&t,0,worker,0)){puts("TIMED_FAIL create");return 1;}
 /* let worker hit first timedwait and time out (200ms), then signal */
 struct timespec rq={0,400*1000000}; nanosleep(&rq,0);
 pthread_mutex_lock(&mtx); ready=1; pthread_cond_signal(&cv); pthread_mutex_unlock(&mtx);
 void *r; if(pthread_join(t,&r)){puts("TIMED_FAIL join");return 1;}
 if(r!=0){puts("TIMED_FAIL worker");return 1;}
 puts("TIMED_OK");
 return 0;
}
