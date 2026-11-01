#include <stdio.h>
int main(int argc, char **argv){
 printf("GLIBC_HELLO argc=%d argv0=%s\n", argc, argv[0]);
 FILE *f=fopen("/etc/motd","r");
 if(f){char b[64];size_t n=fread(b,1,sizeof b-1,f);b[n]=0;printf("GLIBC_MOTD %s",b);fclose(f);}else{printf("GLIBC_NOMOTD\n");}
 return 0;
}
