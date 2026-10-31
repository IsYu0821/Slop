// Minimal fork+execveat+waitpid stress (no block I/O): child execs
// /bin/nopie-helper which exits 17; parent checks status. Repeat N times.
#include <stdio.h>
#include <unistd.h>
#include <sys/wait.h>
#include <sys/syscall.h>
#include <fcntl.h>
#include <stdlib.h>
int main(int argc, char **argv){
 int n = argc > 1 ? atoi(argv[1]) : 20;
 int bfd = open("/bin", O_RDONLY | O_DIRECTORY);
 if(bfd < 0){puts("EXECVEAT_FAIL open");return 1;}
 for(int i=0;i<n;i++){
  pid_t c = fork();
  if(c < 0){puts("EXECVEAT_FAIL fork");return 1;}
  if(c == 0){
   char *args[] = {"nopie-helper", "fixed", 0};
   char *env[] = {"SLOP_MNU_DIFF=1", 0};
   long r = syscall(SYS_execveat, bfd, "nopie-helper", args, env, 0);
   _exit(71);
   (void)r;
  }
  int st;
  if(waitpid(c, &st, 0) != c){puts("EXECVEAT_FAIL wait");return 1;}
  if(!WIFEXITED(st) || WEXITSTATUS(st) != 17){
   printf("EXECVEAT_FAIL iter=%d status=%d wif=%d\n", i, st, WIFEXITED(st));
   return 1;
  }
 }
 puts("EXECVEAT_OK");
 return 0;
}
