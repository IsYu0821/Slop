#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <sys/mman.h>
#include <unistd.h>
#define NM 4
#define SZ (256*1024)
#define ITERS 20
static void *base[NM][ITERS];
static int bad = 0;
static void *w(void *a){
 long n = (long)a;
 for(int i = 0; i < ITERS; i++){
  void *m = mmap(0, SZ, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
  if(m == MAP_FAILED){ printf("MMAP_FAIL\n"); bad=1; return 0; }
  memset(m, (int)(n+1), SZ);
  base[n][i] = m;
 }
 return (void*)n;
}
int main(void){
 printf("MRACE_START\n");
 memset(base, 0, sizeof base);
 pthread_t t[NM];
 for(long i=0;i<NM;i++) if(pthread_create(&t[i],0,w,(void*)i)){printf("CREATE_FAIL\n");return 1;}
 for(int i=0;i<NM;i++) pthread_join(t[i],0);
 if(bad){printf("MRACE_MMAP_FAIL\n");return 1;}
 for(int n = 0; n < NM; n++) for(int i = 0; i < ITERS; i++){
  unsigned char *b = (unsigned char*)base[n][i];
  for(int j = 0; j < SZ; j += 4096){
   if(b[j] != (unsigned char)(n+1)){ printf("MRACE_CORRUPT t=%d it=%d off=%d got=%d\n", n, i, j, b[j]); return 1; }
  }
 }
 for(int n = 0; n < NM; n++) for(int i = 0; i < ITERS; i++){
  unsigned long s1 = (unsigned long)base[n][i], e1 = s1 + SZ;
  for(int m = 0; m < NM; m++) for(int j = 0; j < ITERS; j++){
   if(n==m && i==j) continue;
   unsigned long s2 = (unsigned long)base[m][j], e2 = s2 + SZ;
   if(s1 < e2 && s2 < e1){ printf("MRACE_OVERLAP\n"); return 1; }
  }
 }
 printf("MRACE_OK\n");
 return 0;
}
