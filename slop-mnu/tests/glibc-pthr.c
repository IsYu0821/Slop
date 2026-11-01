#define _GNU_SOURCE
#include <stdio.h>
#include <pthread.h>
#include <dlfcn.h>
#include <string.h>
static int counter=0;
static pthread_mutex_t mtx=PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv=PTHREAD_COND_INITIALIZER;
static int ready=0;
static __thread int tlsval;
void *worker(void *a){
 long id=(long)a; tlsval=(int)(100+id);
 pthread_mutex_lock(&mtx);
 while(!ready) pthread_cond_wait(&cv,&mtx);
 counter++;
 pthread_mutex_unlock(&mtx);
 return (void*)(long)tlsval;
}
typedef int (*addfn)(int,int);
int main(void){
 pthread_t t[4];
 for(long i=0;i<4;i++) if(pthread_create(&t[i],0,worker,(void*)i)){puts("PTHREAD_FAIL create");return 1;}
 pthread_mutex_lock(&mtx); ready=1; pthread_cond_broadcast(&cv); pthread_mutex_unlock(&mtx);
 long sum=0;
 for(int i=0;i<4;i++){void *r; if(pthread_join(t[i],&r)){puts("PTHREAD_FAIL join");return 1;} sum+=(long)r;}
 if(counter!=4){printf("PTHREAD_FAIL count=%d\n",counter);return 1;}
 if(sum!=406){printf("PTHREAD_FAIL tls sum=%ld\n",sum);return 1;}
 /* dlopen test */
 void *h=dlopen("/lib/libtprt.so",RTLD_NOW);
 if(!h){printf("DLOPEN_FAIL %s\n",dlerror());return 1;}
 addfn add=(addfn)dlsym(h,"tprt_add");
 if(!add){puts("DLSYM_FAIL");return 1;}
 if(add(20,22)!=42){puts("DLOPEN_FAIL call");return 1;}
 dlclose(h);
 puts("PTHREAD_OK");
 puts("DLOPEN_OK");
 return 0;
}
