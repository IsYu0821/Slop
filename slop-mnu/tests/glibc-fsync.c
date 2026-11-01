#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
int main(void){
 int fd=open("/tmp/fsync-test.bin",O_CREAT|O_RDWR|O_TRUNC,0644);
 if(fd<0){printf("FSYNC_FAIL open %d\n",errno);return 1;}
 char w[512]; for(int i=0;i<512;i++) w[i]=(char)i;
 if(write(fd,w,512)!=512){puts("FSYNC_FAIL write");return 1;}
 if(fsync(fd)!=0){printf("FSYNC_FAIL fsync %d\n",errno);return 1;}
 if(fdatasync(fd)!=0){printf("FSYNC_FAIL fdatasync %d\n",errno);return 1;}
 if(syncfs(fd)!=0){printf("FSYNC_FAIL syncfs %d\n",errno);return 1;}
 /* sync_file_range with WAIT_AFTER (flush) */
 extern int sync_file_range(int,long long,long long,unsigned);
 if(sync_file_range(fd,0,512,4)!=0){printf("FSYNC_FAIL sfr %d\n",errno);return 1;}
 sync();
 close(fd);
 /* directory fsync */
 int dfd=open("/tmp",O_RDONLY|O_DIRECTORY);
 if(dfd>=0){ if(fsync(dfd)!=0){printf("FSYNC_FAIL dirfsync %d\n",errno);return 1;} close(dfd); }
 /* bad fd must fail */
 if(fsync(9999)==0||errno!=9){puts("FSYNC_FAIL badfd");return 1;}
 /* verify content survived */
 fd=open("/tmp/fsync-test.bin",O_RDONLY);
 if(fd<0){puts("FSYNC_FAIL reopen");return 1;}
 char r[512]={0};
 if(read(fd,r,512)!=512||memcmp(r,w,512)){puts("FSYNC_FAIL reread");return 1;}
 close(fd);
 unlink("/tmp/fsync-test.bin");
 puts("FSYNC_OK");
 return 0;
}
