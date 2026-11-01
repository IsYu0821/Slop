#include <stdio.h>
#include <pthread.h>
static void *worker(void *a){ return (void*)((long)a+1000); }
int main(void){
 pthread_t t;
 if(pthread_create(&t,0,worker,(void*)41)){puts("MIN_FAIL create");return 1;}
 void *r=0;
 if(pthread_join(t,&r)){puts("MIN_FAIL join");return 1;}
 if((long)r!=1041){printf("MIN_FAIL retval=%ld\n",(long)r);return 1;}
 puts("MIN_OK");
 return 0;
}
